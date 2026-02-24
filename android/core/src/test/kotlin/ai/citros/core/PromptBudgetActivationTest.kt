package ai.citros.core

import org.junit.After
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test
import java.time.Instant

/**
 * Tests for PR2: Prompt budget activation and runtime line wiring.
 *
 * Covers:
 * - Feature flag toggle between old and new paths
 * - Tier-specific tool parameter descriptions
 * - RuntimeLine presence in final prompt
 * - Safety contract assertion behavior
 * - Action loop uses MINIMAL mode via budget path
 */
class PromptBudgetActivationTest {

    private val testTimestamp = Instant.parse("2026-02-24T20:00:00Z")

    @Before
    fun setUp() {
        FeatureFlags.resetToDefaults()
    }

    @After
    fun tearDown() {
        FeatureFlags.resetToDefaults()
    }

    // ── Feature flag wiring ─────────────────────────────────────────────

    @Test
    fun `flag off - buildSystemPrompt returns old path without runtime line`() {
        FeatureFlags.promptTuningV1Enabled = false
        val prompt = PhoneAgentPrompts.buildSystemPrompt(
            phoneControlAvailable = true,
            modelName = "gpt-4o"
        )
        // Old path uses "Runtime: model=" format, not "runtime|ts="
        assertFalse("Should not contain structured runtime line", prompt.contains("runtime|ts="))
        assertTrue("Should contain identity", prompt.contains("You are Citros"))
    }

    @Test
    fun `flag on - buildSystemPrompt returns budget-enforced prompt with runtime line`() {
        FeatureFlags.promptTuningV1Enabled = true
        val prompt = PhoneAgentPrompts.buildSystemPrompt(
            phoneControlAvailable = true,
            modelName = "gpt-4o"
        )
        assertTrue("Should contain structured runtime line", prompt.contains("runtime|ts="))
        assertTrue("Should contain safety clauses", prompt.contains("SAFE-001"))
    }

    @Test
    fun `flag off - buildActionPrompt returns old path`() {
        FeatureFlags.promptTuningV1Enabled = false
        val prompt = PhoneAgentPrompts.buildActionPrompt(
            phoneControlAvailable = true,
            modelName = "gpt-4o"
        )
        assertFalse("Should not contain structured runtime line", prompt.contains("runtime|ts="))
    }

    @Test
    fun `flag on - buildActionPrompt uses MINIMAL budget path`() {
        FeatureFlags.promptTuningV1Enabled = true
        val prompt = PhoneAgentPrompts.buildActionPrompt(
            phoneControlAvailable = true,
            modelName = "gpt-4o"
        )
        assertTrue("Should contain structured runtime line", prompt.contains("runtime|ts="))
        assertTrue("Should contain mode=MINIMAL", prompt.contains("mode=MINIMAL"))
    }

    // ── RuntimeLine presence ────────────────────────────────────────────

    @Test
    fun `runtime line is present and parseable in tuned prompt`() {
        FeatureFlags.promptTuningV1Enabled = true
        val prompt = PhoneAgentPrompts.buildSystemPrompt(
            phoneControlAvailable = true,
            modelName = "claude-sonnet-4-20250514",
            modelTier = ModelTier.STANDARD
        )
        val runtimeLine = prompt.lineSequence().lastOrNull { it.startsWith("runtime|") }
        assertNotNull("Runtime line should be present", runtimeLine)
        val parsed = RuntimeLine.parse(runtimeLine!!)
        assertNotNull("Runtime line should be parseable", parsed)
        assertEquals("STANDARD", parsed!!.tier)
        assertEquals("FULL", parsed.mode)
        assertEquals("attached", parsed.accessibility)
    }

    @Test
    fun `runtime line present in MINIMAL action prompt`() {
        FeatureFlags.promptTuningV1Enabled = true
        val prompt = PhoneAgentPrompts.buildActionPrompt(
            phoneControlAvailable = true,
            modelName = "claude-haiku-3"
        )
        val runtimeLine = prompt.lineSequence().lastOrNull { it.startsWith("runtime|") }
        assertNotNull("Runtime line should be in action prompt", runtimeLine)
        val parsed = RuntimeLine.parse(runtimeLine!!)
        assertNotNull(parsed)
        assertEquals("MINIMAL", parsed!!.mode)
    }

    // ── Tier-specific tool parameter descriptions ───────────────────────

    @Test
    fun `FLAGSHIP tier gets full tool parameter detail`() {
        FeatureFlags.promptTuningV1Enabled = true
        val result = PhoneAgentPrompts.buildTunedSystemPrompt(
            phoneControlAvailable = true,
            modelName = "claude-opus-4-6",
            modelTier = ModelTier.FLAGSHIP,
            mode = PromptMode.FULL,
            timestamp = testTimestamp
        )
        assertTrue(
            "FLAGSHIP should get full tool parameter detail",
            result.finalPrompt.contains("Always supply exact, current arguments")
        )
    }

    @Test
    fun `STANDARD tier gets full tool parameter detail`() {
        FeatureFlags.promptTuningV1Enabled = true
        val result = PhoneAgentPrompts.buildTunedSystemPrompt(
            phoneControlAvailable = true,
            modelName = "gpt-4o",
            modelTier = ModelTier.STANDARD,
            mode = PromptMode.FULL,
            timestamp = testTimestamp
        )
        assertTrue(
            "STANDARD should get full tool parameter detail",
            result.finalPrompt.contains("Always supply exact, current arguments")
        )
    }

    @Test
    fun `SMALL tier gets abbreviated tool parameter detail`() {
        FeatureFlags.promptTuningV1Enabled = true
        val result = PhoneAgentPrompts.buildTunedSystemPrompt(
            phoneControlAvailable = true,
            modelName = "gemini-flash",
            modelTier = ModelTier.SMALL,
            mode = PromptMode.FULL,
            timestamp = testTimestamp
        )
        assertFalse(
            "SMALL should NOT get full tool parameter detail",
            result.finalPrompt.contains("Always supply exact, current arguments")
        )
        assertTrue(
            "SMALL should include abbreviated tool parameter detail content",
            result.finalPrompt.contains("wait: 1-5s, then re-check")
        )
    }

    @Test
    fun `SMALL tier abbreviated detail is shorter than full`() {
        assertTrue(
            "Abbreviated should be shorter",
            PhoneAgentPrompts.SECTION_TOOL_PARAMETER_DETAIL_SMALL.length <
                PhoneAgentPrompts.SECTION_TOOL_PARAMETER_DETAIL.length
        )
    }

    // ── Safety contract assertion ───────────────────────────────────────

    @Test
    fun `safety contract assertion fires on missing clause`() {
        val promptMissingSafety = "You are Citros. No safety clauses here."
        val missing = PromptSafetyContract.findMissingClauses(promptMissingSafety)
        assertTrue("Should detect missing clauses", missing.isNotEmpty())
        assertEquals(4, missing.size)
    }

    @Test
    fun `safety contract passes on valid tuned prompt`() {
        FeatureFlags.promptTuningV1Enabled = true
        val prompt = PhoneAgentPrompts.buildSystemPrompt(
            phoneControlAvailable = true,
            modelName = "gpt-4o"
        )
        val missing = PromptSafetyContract.findMissingClauses(prompt)
        assertTrue("No clauses should be missing: $missing", missing.isEmpty())
    }

    @Test
    fun `safety contract passes on MINIMAL tuned prompt`() {
        FeatureFlags.promptTuningV1Enabled = true
        val prompt = PhoneAgentPrompts.buildActionPrompt(
            phoneControlAvailable = true,
            modelName = "gpt-4o"
        )
        val missing = PromptSafetyContract.findMissingClauses(prompt)
        assertTrue("No clauses should be missing in MINIMAL: $missing", missing.isEmpty())
    }

    // ── Budget metadata ─────────────────────────────────────────────────

    @Test
    fun `budget result includes token estimate and char count`() {
        FeatureFlags.promptTuningV1Enabled = true
        val result = PhoneAgentPrompts.buildTunedSystemPrompt(
            phoneControlAvailable = true,
            modelName = "gpt-4o",
            mode = PromptMode.FULL,
            timestamp = testTimestamp
        )
        assertTrue("charCount should be positive", result.charCount > 0)
        assertTrue("tokenEstimate should be positive", result.tokenEstimate > 0)
        assertEquals(PromptBudget.estimateTokens(result.charCount), result.tokenEstimate)
    }

    // ── Regression: old path unchanged when flag off ────────────────────

    @Test
    fun `old path prompt content unchanged when flag off`() {
        FeatureFlags.promptTuningV1Enabled = false
        val oldPrompt = PhoneAgentPrompts.buildSystemPrompt(
            phoneControlAvailable = true,
            modelName = "gpt-4o",
            modelTier = ModelTier.STANDARD
        )
        // Old path should have the traditional runtime format
        assertTrue("Old path should have identity", oldPrompt.contains("You are Citros"))
        assertTrue("Old path should have tools", oldPrompt.contains("## Your Tools"))
        assertTrue("Old path should have old runtime format", oldPrompt.contains("Runtime: model=gpt-4o"))
        // Should NOT have budget-enforced artifacts
        assertFalse("Old path should not have safety clauses block", oldPrompt.contains("Canonical Safety Clauses"))
    }

    private fun assertNotNull(message: String, value: Any?) {
        assertFalse(message, value == null)
    }
}
