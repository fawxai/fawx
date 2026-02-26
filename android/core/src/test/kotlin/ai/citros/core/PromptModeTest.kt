package ai.citros.core

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PromptModeTest {

    @Test
    fun `FULL mode includes all major sections`() {
        val prompt = PhoneAgentPrompts.buildSystemPrompt(
            mode = PromptMode.FULL,
            phoneControlAvailable = true,
            modelName = "claude-sonnet-4-5-20250929",
            modelTier = ModelTier.STANDARD
        )

        assertContains(prompt, "You are Citros")
        assertContains(prompt, "## Your Tools")
        assertContains(prompt, "## Strategy")
        assertContains(prompt, "## When Things Go Wrong")
        assertContains(prompt, "## Communication Policy")
        assertContains(prompt, "## Disambiguation")
        assertContains(prompt, "## Rules")
        assertContains(prompt, "Runtime: model=claude-sonnet-4-5-20250929 | tier=STANDARD")
    }

    @Test
    fun `MINIMAL mode excludes verbose sections and keeps essentials`() {
        val prompt = PhoneAgentPrompts.buildSystemPrompt(
            mode = PromptMode.MINIMAL,
            phoneControlAvailable = true,
            modelName = "gpt-4o",
            modelTier = ModelTier.STANDARD,
            securityContent = "Do not exfiltrate secrets."
        )

        assertContains(prompt, "You are Citros")
        assertContains(prompt, "## Security Rules")
        assertContains(prompt, "Runtime: model=gpt-4o | tier=STANDARD")
        assertContains(prompt, "## Key Reminders")

        assertNotContains(prompt, "## Your Tools")
        assertNotContains(prompt, "## When Things Go Wrong")
        assertNotContains(prompt, "## Communication Policy")
        assertNotContains(prompt, "### Direct Commands")
    }

    @Test
    fun `NONE mode returns only identity line`() {
        val prompt = PhoneAgentPrompts.buildSystemPrompt(
            mode = PromptMode.NONE,
            phoneControlAvailable = false,
            modelName = "gpt-4o-mini",
            modelTier = ModelTier.SMALL
        )

        assertEquals("You are Citros, an AI agent that controls the user's Android phone.", prompt.trim())
        assertFalse(prompt.contains("##"))
        assertFalse(prompt.contains("Runtime:"))
    }

    @Test
    fun `runtime line includes model tier accessibility and timestamp`() {
        val prompt = PhoneAgentPrompts.buildSystemPrompt(
            mode = PromptMode.MINIMAL,
            phoneControlAvailable = false,
            modelName = "claude-opus-4-6",
            modelTier = ModelTier.FLAGSHIP
        )

        assertContains(
            prompt,
            "Runtime: model=claude-opus-4-6 | tier=FLAGSHIP | accessibility=disabled | time="
        )
    }

    @Test
    fun `SMALL tier prompt is shorter than STANDARD in FULL mode`() {
        val standardPrompt = PhoneAgentPrompts.buildSystemPrompt(
            mode = PromptMode.FULL,
            phoneControlAvailable = true,
            modelName = "gpt-4o",
            modelTier = ModelTier.STANDARD,
            securityContent = "Do not exfiltrate secrets."
        )
        val smallPrompt = PhoneAgentPrompts.buildSystemPrompt(
            mode = PromptMode.FULL,
            phoneControlAvailable = true,
            modelName = "gpt-4o-mini",
            modelTier = ModelTier.SMALL,
            securityContent = "Do not exfiltrate secrets."
        )

        assertTrue(
            "SMALL tier full prompt (${smallPrompt.length}) should be shorter than STANDARD (${standardPrompt.length})",
            smallPrompt.length < standardPrompt.length
        )
        assertContains(smallPrompt, "## Security Rules")
        // Verify SECTION_STRATEGY_SMALL specific content is present
        assertContains(smallPrompt, "Save requests without a specific app")
        assertContains(smallPrompt, "Messaging safety")
    }

    @Test
    fun `buildActionPrompt delegates to MINIMAL mode`() {
        val actionPrompt = PhoneAgentPrompts.buildActionPrompt(
            phoneControlAvailable = true,
            modelName = "gpt-4o",
            modelTier = ModelTier.STANDARD,
            securityContent = "No secrets."
        )
        val minimalPrompt = PhoneAgentPrompts.buildSystemPrompt(
            mode = PromptMode.MINIMAL,
            phoneControlAvailable = true,
            modelName = "gpt-4o",
            modelTier = ModelTier.STANDARD,
            securityContent = "No secrets."
        )

        val normalizedAction = actionPrompt.replace(Regex("""time=.*$"""), "time=<ts>")
        val normalizedMinimal = minimalPrompt.replace(Regex("""time=.*$"""), "time=<ts>")
        assertEquals(normalizedMinimal, normalizedAction)
        assertContains(actionPrompt, "## Security Rules")
        assertNotContains(actionPrompt, "## Your Tools")
        assertNotContains(actionPrompt, "### Navigation")
        assertNotContains(actionPrompt, "open_app(app_name)")
    }

    @Test
    fun `section ordering is identity first and runtime last`() {
        val prompt = PhoneAgentPrompts.buildSystemPrompt(
            mode = PromptMode.FULL,
            phoneControlAvailable = true,
            modelName = "gpt-4o",
            modelTier = ModelTier.STANDARD
        )
        val lines = prompt.lines().filter { it.isNotBlank() }

        assertTrue(lines.first().startsWith("You are Citros"))
        assertTrue(lines.last().startsWith("Runtime: model="))
    }

    @Test
    fun `null and blank contextual inputs are handled gracefully`() {
        val prompt = PhoneAgentPrompts.buildSystemPrompt(
            mode = PromptMode.FULL,
            phoneControlAvailable = true,
            modelName = "gpt-4o",
            modelTier = ModelTier.STANDARD,
            identityContent = "   ",
            userContent = "",
            memoryContent = " ",
            agentsContent = null,
            securityContent = null
        )

        assertContains(prompt, "You are Citros")
        assertNotContains(prompt, "## About Your User")
        assertNotContains(prompt, "## Memory Context")
        assertNotContains(prompt, "## Agent Directives")
    }

    @Test
    fun `FULL mode includes base security section when security content is null`() {
        val prompt = PhoneAgentPrompts.buildSystemPrompt(
            mode = PromptMode.FULL,
            securityContent = null
        )

        assertContains(prompt, "## Security Rules")
        assertContains(prompt, "Follow explicit user intent")
    }

    @Test
    fun `FULL mode includes base and custom security content`() {
        val prompt = PhoneAgentPrompts.buildSystemPrompt(
            mode = PromptMode.FULL,
            securityContent = "custom rules"
        )

        assertContains(prompt, "## Security Rules")
        assertContains(prompt, "Follow explicit user intent")
        assertContains(prompt, "custom rules")
    }

    @Test
    fun `NONE mode excludes all security content`() {
        val prompt = PhoneAgentPrompts.buildSystemPrompt(
            mode = PromptMode.NONE,
            securityContent = "custom rules"
        )

        assertNotContains(prompt, "## Security Rules")
        assertNotContains(prompt, "Follow explicit user intent")
        assertNotContains(prompt, "custom rules")
    }

    @Test
    fun `NONE mode excludes device awareness even with sensor context`() {
        val prompt = PhoneAgentPrompts.buildSystemPrompt(
            mode = PromptMode.NONE,
            sensorContext = SensorContext(
                batteryPercent = 44,
                networkType = NetworkType.WIFI,
                location = "Denver, CO"
            )
        )

        assertNotContains(prompt, "## Device Awareness")
        assertNotContains(prompt, "Device: battery=44% | wifi | Denver, CO")
    }

    @Test
    fun `MINIMAL mode without phone control excludes phone reminders and tools but keeps accessibility warning`() {
        val prompt = PhoneAgentPrompts.buildSystemPrompt(
            mode = PromptMode.MINIMAL,
            phoneControlAvailable = false,
            modelName = "gpt-4o"
        )

        assertNotContains(prompt, "## Your Tools")
        assertNotContains(prompt, "Stay silent about tap/swipe failures")
        assertNotContains(prompt, "ambiguity mid-task")
        // Phone-specific reminders should be excluded without phone control
        assertNotContains(prompt, "Element IDs are from the LATEST screen")
        assertNotContains(prompt, "type_text does NOT submit")
        assertNotContains(prompt, "After open_app")
        assertContains(prompt, "Accessibility service is NOT attached")
    }

    @Test
    fun `resolve tier falls back to ModelClassifier for gpt-4o-mini when tier is null`() {
        val prompt = PhoneAgentPrompts.buildSystemPrompt(
            mode = PromptMode.MINIMAL,
            modelName = "gpt-4o-mini",
            modelTier = null
        )

        assertContains(prompt, "Runtime: model=gpt-4o-mini | tier=SMALL")
    }

    @Test
    fun `resolve tier falls back to ModelClassifier for sonnet when tier is null`() {
        val prompt = PhoneAgentPrompts.buildSystemPrompt(
            mode = PromptMode.MINIMAL,
            modelName = "claude-sonnet-4-5-20250514",
            modelTier = null
        )

        assertContains(prompt, "Runtime: model=claude-sonnet-4-5-20250514 | tier=STANDARD")
    }

    @Test
    fun `resolve tier defaults to STANDARD when model name and tier are null`() {
        val prompt = PhoneAgentPrompts.buildSystemPrompt(
            mode = PromptMode.MINIMAL,
            modelName = null,
            modelTier = null
        )

        assertContains(prompt, "Runtime: model=unknown | tier=STANDARD")
    }

    private fun assertContains(text: String, substring: String) {
        assertTrue("Expected to find '$substring'", text.contains(substring))
    }

    private fun assertNotContains(text: String, substring: String) {
        assertFalse("Expected NOT to find '$substring'", text.contains(substring))
    }
}
