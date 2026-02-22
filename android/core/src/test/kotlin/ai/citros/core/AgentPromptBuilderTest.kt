package ai.citros.core

import org.junit.After
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.shadows.ShadowLog
import kotlin.test.assertFalse
import kotlin.test.assertTrue

@RunWith(RobolectricTestRunner::class)
class AgentPromptBuilderTest {
    private val tempRoot = createTempDir(prefix = "agent-prompt-builder-test")

    @After
    fun tearDown() {
        tempRoot.deleteRecursively()
    }

    @Test
    fun `full includes all available sections and trimmed includes soul and security only`() {
        val manager = AgentFileManager.fromDirectory(tempRoot)
        manager.writeFile("SOUL.md", "I am Citros")
        manager.writeFile("USER.md", "User is Joe")
        manager.writeFile("TOOLS.md", "tool notes")

        val builder = AgentPromptBuilder(manager)
        val full = builder.full()
        val trimmed = builder.trimmed()

        // Full prompt weaves identity files into phone agent sections
        assertTrue(full.contains("I am Citros"), "Full should contain SOUL.md content")
        assertTrue(full.contains("User is Joe"), "Full should contain USER.md content")
        assertTrue(full.contains("## Strategy"), "Full should contain phone agent strategy")
        assertTrue(full.contains("## Agent Directives"), "Full should contain AGENTS.md as directives")

        // Trimmed is the action loop prompt with security rules
        assertTrue(trimmed.contains("Continue executing"), "Trimmed should be action prompt")
        assertTrue(trimmed.contains("Security Rules"), "Trimmed should include security rules")
        assertFalse(trimmed.contains("User is Joe"), "Trimmed should not include user content")
    }

    @Test
    fun `missing files are handled gracefully`() {
        val manager = AgentFileManager.fromDirectory(tempRoot)
        val builder = AgentPromptBuilder(manager)

        val full = builder.full()
        val trimmed = builder.trimmed()

        assertTrue(full.isNotBlank(), "Full prompt should not be blank")
        assertTrue(trimmed.isNotBlank(), "Trimmed prompt should not be blank")
        // With no SOUL.md content, falls back to default identity
        assertTrue(full.contains("You are Citros"), "Should fall back to default identity")
        assertTrue(full.contains("## Strategy"), "Should contain strategy section")
    }

    @Test
    fun `skipped sections are logged when file is missing`() {
        ShadowLog.reset()
        val manager = AgentFileManager.fromDirectory(tempRoot)
        // USER.md, TOOLS.md, MEMORY.md are not created by initializeDefaults
        val builder = AgentPromptBuilder(manager)
        builder.full()

        val logs = ShadowLog.getLogsForTag("AgentPromptBuilder")
        val skippedFiles = logs.map { it.msg }
        assertTrue(skippedFiles.any { it.contains("Skipping USER.md: not readable") },
            "Expected log for skipped USER.md, got: $skippedFiles")
        // MEMORY.md is read via readMemoryForPrompt() which handles errors silently
    }

    @Test
    fun `blank files are logged as skipped`() {
        ShadowLog.reset()
        val manager = AgentFileManager.fromDirectory(tempRoot)
        // SOUL.md is created empty by initializeDefaults
        val builder = AgentPromptBuilder(manager)
        builder.full()

        val logs = ShadowLog.getLogsForTag("AgentPromptBuilder")
        val skippedFiles = logs.map { it.msg }
        assertTrue(skippedFiles.any { it.contains("Skipping SOUL.md: blank or whitespace-only") },
            "Expected log for blank SOUL.md, got: $skippedFiles")
    }

    @Test
    fun `full passes sensorContext through to composed prompt`() {
        val manager = AgentFileManager.fromDirectory(tempRoot)
        val builder = AgentPromptBuilder(manager)

        val full = builder.full(
            sensorContext = SensorContext(batteryPercent = 83, networkType = NetworkType.WIFI)
        )

        assertTrue(full.contains("Device Awareness"))
        assertTrue(full.contains("Device: battery=83% | wifi"))
    }
}
