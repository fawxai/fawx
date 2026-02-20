package ai.citros.core

import org.junit.Assert.*
import org.junit.Test

class PhoneAgentPromptsTest {

    // ── buildSystemPrompt ───────────────────────────────────────────────

    @Test
    fun `buildSystemPrompt with phone control includes tool categories`() {
        val prompt = PhoneAgentPrompts.buildSystemPrompt(phoneControlAvailable = true)
        assertContains(prompt, "## Your Tools")
        assertContains(prompt, "### Navigation")
        assertContains(prompt, "### Interaction")
        assertContains(prompt, "### Observation")
        assertContains(prompt, "### Notifications")
        assertContains(prompt, "### Clipboard")
        assertContains(prompt, "### Memory & Files")
        assertContains(prompt, "### Planning")
    }

    @Test
    fun `buildSystemPrompt without phone control omits tools section`() {
        val prompt = PhoneAgentPrompts.buildSystemPrompt(phoneControlAvailable = false)
        assertNotContains(prompt, "## Your Tools")
        assertNotContains(prompt, "### Navigation")
        assertNotContains(prompt, "open_app(app_name)")
    }

    @Test
    fun `buildSystemPrompt without phone control includes accessibility warning`() {
        val prompt = PhoneAgentPrompts.buildSystemPrompt(phoneControlAvailable = false)
        assertContains(prompt, "Accessibility service is NOT attached")
        assertContains(prompt, "Phone control unavailable")
        assertContains(prompt, "Accessibility: disabled")
    }

    @Test
    fun `buildSystemPrompt with phone control shows attached status`() {
        val prompt = PhoneAgentPrompts.buildSystemPrompt(phoneControlAvailable = true)
        assertContains(prompt, "Accessibility: enabled")
        assertNotContains(prompt, "Accessibility service is NOT attached")
    }

    @Test
    fun `buildSystemPrompt includes model name when provided`() {
        val prompt = PhoneAgentPrompts.buildSystemPrompt(
            phoneControlAvailable = true,
            modelName = "claude-opus-4-6"
        )
        assertContains(prompt, "Model: claude-opus-4-6")
    }

    @Test
    fun `buildSystemPrompt omits model line when null`() {
        val prompt = PhoneAgentPrompts.buildSystemPrompt(
            phoneControlAvailable = true,
            modelName = null
        )
        assertNotContains(prompt, "Model:")
    }

    // ── Core sections always present ────────────────────────────────────

    @Test
    fun `buildSystemPrompt always includes identity section`() {
        val withControl = PhoneAgentPrompts.buildSystemPrompt(phoneControlAvailable = true)
        val withoutControl = PhoneAgentPrompts.buildSystemPrompt(phoneControlAvailable = false)
        assertContains(withControl, "You are Citros")
        assertContains(withoutControl, "You are Citros")
    }

    @Test
    fun `buildSystemPrompt always includes strategy section`() {
        val prompt = PhoneAgentPrompts.buildSystemPrompt(phoneControlAvailable = true)
        assertContains(prompt, "## Strategy")
        assertContains(prompt, "### Direct Commands")
        assertContains(prompt, "### Tasks")
    }

    @Test
    fun `buildSystemPrompt always includes recovery section`() {
        val prompt = PhoneAgentPrompts.buildSystemPrompt(phoneControlAvailable = true)
        assertContains(prompt, "## When Things Go Wrong")
        assertContains(prompt, "Tap didn't work")
        assertContains(prompt, "stuck")
    }

    @Test
    fun `buildSystemPrompt always includes disambiguation section`() {
        val prompt = PhoneAgentPrompts.buildSystemPrompt(phoneControlAvailable = true)
        assertContains(prompt, "## Disambiguation")
        assertContains(prompt, "Android Settings")
    }

    @Test
    fun `buildSystemPrompt always includes rules section`() {
        val prompt = PhoneAgentPrompts.buildSystemPrompt(phoneControlAvailable = true)
        assertContains(prompt, "## Rules")
        assertContains(prompt, "type_text only enters text")
    }

    @Test
    fun `buildSystemPrompt always includes runtime section`() {
        val prompt = PhoneAgentPrompts.buildSystemPrompt(phoneControlAvailable = true)
        assertContains(prompt, "## Runtime")
        assertContains(prompt, "Time:")
    }

    // ── Strategy teaches key behaviors ──────────────────────────────────

    @Test
    fun `type_text does not submit is reinforced in tools and rules`() {
        assertContains(PhoneAgentPrompts.SECTION_TOOLS, "does NOT submit")
        assertContains(PhoneAgentPrompts.SECTION_RULES, "type_text only enters text")
    }

    @Test
    fun `rules warn against typing app name after open_app`() {
        // Full-sentence assertion: verifies the complete guidance including the concrete example
        assertContains(
            PhoneAgentPrompts.SECTION_RULES,
            "After open_app, type the user's actual query — not the app name you just opened."
        )
        assertContains(
            PhoneAgentPrompts.SECTION_RULES,
            "open_app(\"Google\") then type_text(\"weather in Denver\"), NOT type_text(\"Google\")"
        )
    }

    @Test
    fun `action prompt reinforces not typing app name`() {
        val prompt = PhoneAgentPrompts.buildActionPrompt()
        // Both prompts use consistent wording: "not the app name you just opened"
        assertContains(prompt, "not the app name you just opened")
    }

    // Note: Behavioral validation (does the agent actually stop typing "google"?)
    // requires integration tests with a live LLM, which is beyond unit test scope.
    // These tests verify the guidance is present in the prompt text.

    @Test
    fun `strategy section teaches efficiency - dont read_screen after actions`() {
        assertContains(
            PhoneAgentPrompts.SECTION_STRATEGY,
            "Don't call read_screen after actions"
        )
    }

    @Test
    fun `strategy section teaches direct commands need no observation`() {
        assertContains(PhoneAgentPrompts.SECTION_STRATEGY, "One tool call. No observation needed.")
    }

    @Test
    fun `strategy section teaches confidence-gated disambiguation`() {
        assertContains(PhoneAgentPrompts.SECTION_STRATEGY, "When Uncertain")
        assertContains(PhoneAgentPrompts.SECTION_STRATEGY, "think()")
        assertContains(PhoneAgentPrompts.SECTION_STRATEGY, "confidently resolve")
        assertContains(PhoneAgentPrompts.SECTION_STRATEGY, "Low stakes")
        assertContains(PhoneAgentPrompts.SECTION_STRATEGY, "High stakes")
        // Clarifies that "use context" means existing info, not exploring
        assertContains(PhoneAgentPrompts.SECTION_STRATEGY, "existing context")
        assertContains(PhoneAgentPrompts.SECTION_STRATEGY, "Do NOT open apps")
        // Messaging-specific contact verification rule
        assertContains(PhoneAgentPrompts.SECTION_STRATEGY, "Messaging rule")
        assertContains(PhoneAgentPrompts.SECTION_STRATEGY, "confident you have the right person")
        assertContains(PhoneAgentPrompts.SECTION_STRATEGY, "don't search and pick one")
    }

    @Test
    fun `rules reinforce think-then-ask for uncertain situations`() {
        assertContains(
            PhoneAgentPrompts.SECTION_RULES,
            "unsure what the user wants, use your think() tool"
        )
        assertContains(PhoneAgentPrompts.SECTION_RULES, "high-stakes")
        assertContains(PhoneAgentPrompts.SECTION_RULES, "Never open apps")
    }

    @Test
    fun `action prompt includes mid-task disambiguation reminder`() {
        val prompt = PhoneAgentPrompts.buildActionPrompt(phoneControlAvailable = true)
        assertContains(prompt, "ambiguity mid-task")
        assertContains(prompt, "stop and ask the user")
        // Intentional: action prompt tells agent to ask directly, without think() nudge.
        // think() is still taught in SECTION_STRATEGY (system prompt) — no need to repeat here.
        assertNotContains(prompt, "think()")
    }

    @Test
    fun `action prompt omits disambiguation reminder when phone control disabled`() {
        val prompt = PhoneAgentPrompts.buildActionPrompt(phoneControlAvailable = false)
        assertNotContains(prompt, "ambiguity mid-task")
    }

    // ── Recovery teaches failure patterns ────────────────────────────────

    @Test
    fun `recovery section covers stuck detection`() {
        assertContains(PhoneAgentPrompts.SECTION_RECOVERY, "Screen hasn't changed after 2 actions")
    }

    @Test
    fun `recovery section covers keyboard blocking`() {
        assertContains(PhoneAgentPrompts.SECTION_RECOVERY, "Keyboard blocking")
    }

    @Test
    fun `recovery section covers web search failure with anti-browser directive`() {
        assertContains(PhoneAgentPrompts.SECTION_RECOVERY, "Web search failed")
        assertContains(PhoneAgentPrompts.SECTION_RECOVERY, "Do NOT open Chrome")
    }

    @Test
    fun `recovery section covers web browse failure`() {
        assertContains(PhoneAgentPrompts.SECTION_RECOVERY, "Web browse failed")
    }

    @Test
    fun `strategy section teaches web tool selection`() {
        assertContains(PhoneAgentPrompts.SECTION_STRATEGY, "Search vs Browse vs Chrome")
        assertContains(PhoneAgentPrompts.SECTION_STRATEGY, "Need information")
        assertContains(PhoneAgentPrompts.SECTION_STRATEGY, "Need web interaction")
        assertContains(PhoneAgentPrompts.SECTION_STRATEGY, "Never open Chrome just to search")
    }

    // ── buildActionPrompt ───────────────────────────────────────────────

    @Test
    fun `buildActionPrompt is concise`() {
        val prompt = PhoneAgentPrompts.buildActionPrompt()
        // Action prompt should be much shorter than system prompt
        val systemPrompt = PhoneAgentPrompts.buildSystemPrompt()
        assertTrue(
            "Action prompt (${prompt.length}) should be much shorter than system prompt (${systemPrompt.length})",
            prompt.length < systemPrompt.length / 2
        )
    }

    @Test
    fun `buildActionPrompt contains key reminders`() {
        val prompt = PhoneAgentPrompts.buildActionPrompt()
        assertContains(prompt, "Element IDs")
        assertContains(prompt, "type_text does NOT submit")
        assertContains(prompt, "screen hasn't changed")
        assertContains(prompt, "text only")
    }

    @Test
    fun `buildActionPrompt includes model name when provided`() {
        val prompt = PhoneAgentPrompts.buildActionPrompt(modelName = "gpt-4o")
        assertContains(prompt, "Model: gpt-4o")
    }

    @Test
    fun `buildActionPrompt omits model line when null`() {
        val prompt = PhoneAgentPrompts.buildActionPrompt(modelName = null)
        assertNotContains(prompt, "Model:")
    }

    // ── Communication Policy ────────────────────────────────────────────

    @Test
    fun `buildSystemPrompt includes communication policy when phone control available`() {
        val prompt = PhoneAgentPrompts.buildSystemPrompt(phoneControlAvailable = true)
        assertContains(prompt, "## Communication Policy")
        assertContains(prompt, "Stay silent about")
        assertContains(prompt, "Alert the user about")
    }

    @Test
    fun `buildSystemPrompt omits communication policy when phone control disabled`() {
        val prompt = PhoneAgentPrompts.buildSystemPrompt(phoneControlAvailable = false)
        assertNotContains(prompt, "## Communication Policy")
    }

    @Test
    fun `communication policy section covers key guidelines`() {
        assertContains(PhoneAgentPrompts.SECTION_COMMUNICATION, "tap/swipe failures")
        assertContains(PhoneAgentPrompts.SECTION_COMMUNICATION, "app not installed")
        assertContains(PhoneAgentPrompts.SECTION_COMMUNICATION, "accessibility turned off")
        assertContains(PhoneAgentPrompts.SECTION_COMMUNICATION, "3 attempts")
        assertContains(PhoneAgentPrompts.SECTION_COMMUNICATION, "Never show the user")
    }

    @Test
    fun `action prompt includes communication reminder when phone control available`() {
        val prompt = PhoneAgentPrompts.buildActionPrompt(phoneControlAvailable = true)
        assertContains(prompt, "Stay silent about tap/swipe failures")
    }

    @Test
    fun `action prompt omits communication reminder when phone control disabled`() {
        val prompt = PhoneAgentPrompts.buildActionPrompt(phoneControlAvailable = false)
        assertNotContains(prompt, "Stay silent about tap/swipe failures")
    }

    @Test
    fun `communication policy is placed between recovery and disambiguation`() {
        val prompt = PhoneAgentPrompts.buildSystemPrompt(phoneControlAvailable = true)
        val recoveryIndex = prompt.indexOf("## When Things Go Wrong")
        val communicationIndex = prompt.indexOf("## Communication Policy")
        val disambiguationIndex = prompt.indexOf("## Disambiguation")
        assertTrue("Communication Policy should come after Recovery",
            communicationIndex > recoveryIndex)
        assertTrue("Communication Policy should come before Disambiguation",
            communicationIndex < disambiguationIndex)
    }

    // ── Legacy compatibility ────────────────────────────────────────────

    @Test
    fun `SYSTEM_PROMPT lazy val matches buildSystemPrompt default`() {
        // SYSTEM_PROMPT is a lazy val for backward compat
        val lazy = PhoneAgentPrompts.SYSTEM_PROMPT
        assertContains(lazy, "You are Citros")
        assertContains(lazy, "## Strategy")
        assertContains(lazy, "## Runtime")
    }

    @Test
    fun `ACTION_PROMPT lazy val matches buildActionPrompt default`() {
        val lazy = PhoneAgentPrompts.ACTION_PROMPT
        assertContains(lazy, "Continue executing the task")
    }

    // ── Helpers ─────────────────────────────────────────────────────────

    private fun assertContains(text: String, substring: String) {
        assertTrue(
            "Expected to find '$substring' in text:\n${text.take(200)}...",
            text.contains(substring)
        )
    }

    private fun assertNotContains(text: String, substring: String) {
        assertFalse(
            "Expected NOT to find '$substring' in text:\n${text.take(200)}...",
            text.contains(substring)
        )
    }

    // --- Execution rule tests (#613) ---

    @Test
    fun `system prompt contains act-dont-announce rule`() {
        val prompt = PhoneAgentPrompts.buildSystemPrompt(phoneControlAvailable = true)
        assertContains(prompt, "Never announce an action without doing it")
        assertContains(prompt, "Text without tools = conversation, not action")
    }

    @Test
    fun `execution rule is in system prompt only, not action prompt`() {
        val actionPrompt = PhoneAgentPrompts.buildActionPrompt(phoneControlAvailable = true)
        // Action prompt is trimmed for mid-loop use — execution rule is not needed there
        // because the model is already in a tool loop (it IS acting).
        assert(!actionPrompt.contains("Act, Don't Announce")) {
            "Action prompt should not contain the execution rule (it's for initial turns only)"
        }

        // Verify it IS in the system prompt
        val systemPrompt = PhoneAgentPrompts.buildSystemPrompt(phoneControlAvailable = true)
        assertContains(systemPrompt, "Act, Don't Announce")
    }
}
