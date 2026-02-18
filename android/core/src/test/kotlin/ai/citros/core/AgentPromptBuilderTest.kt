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

        assertTrue(full.contains("## SOUL.md"))
        assertTrue(full.contains("## USER.md"))
        assertTrue(full.contains("## AGENTS.md"))
        assertTrue(full.contains("## SECURITY.md"))
        assertTrue(full.contains("## TOOLS.md"))

        assertTrue(trimmed.contains("## SOUL.md"))
        assertTrue(trimmed.contains("## SECURITY.md"))
        assertFalse(trimmed.contains("## USER.md"))
        assertFalse(trimmed.contains("## TOOLS.md"))
    }

    @Test
    fun `missing files are handled gracefully`() {
        val manager = AgentFileManager.fromDirectory(tempRoot)
        val builder = AgentPromptBuilder(manager)

        val full = builder.full()
        val trimmed = builder.trimmed()

        assertTrue(full.isNotBlank())
        assertTrue(trimmed.isNotBlank())
        assertTrue(trimmed.contains("## SECURITY.md"))
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
        assertTrue(skippedFiles.any { it.contains("Skipping section USER.md: not readable") },
            "Expected log for skipped USER.md, got: $skippedFiles")
        assertTrue(skippedFiles.any { it.contains("Skipping section MEMORY.md: not readable") },
            "Expected log for skipped MEMORY.md, got: $skippedFiles")
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
        assertTrue(skippedFiles.any { it.contains("Skipping section SOUL.md: blank or whitespace-only") },
            "Expected log for blank SOUL.md, got: $skippedFiles")
    }
}
