package ai.citros.core

import org.junit.After
import org.junit.Test
import java.io.File
import java.nio.file.Files
import java.time.LocalDate
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

class AgentFileManagerTest {

    private val tempRoot = createTempDir(prefix = "agent-file-manager-test")

    @After
    fun tearDown() {
        tempRoot.deleteRecursively()
    }

    @Test
    fun `read write list works in scoped directory`() {
        val manager = AgentFileManager.fromDirectory(tempRoot)

        manager.writeFile("memory/2026-02-12.md", "hello memory")
        val read = manager.readFile("memory/2026-02-12.md")
        val list = manager.listFiles("memory")

        assertEquals("hello memory", read)
        assertEquals(listOf("memory/2026-02-12.md"), list)
        assertTrue(File(tempRoot, "SOUL.md").exists(), "SOUL.md should exist by default")
    }

    @Test
    fun `path traversal is blocked`() {
        val manager = AgentFileManager.fromDirectory(tempRoot)

        assertFailsWith<SecurityException> {
            manager.readFile("../outside.md")
        }
        assertFailsWith<SecurityException> {
            manager.writeFile("../../etc/passwd", "nope")
        }
        assertFailsWith<SecurityException> {
            manager.listFiles("../")
        }
    }

    @Test
    fun `symlink escape is blocked`() {
        val manager = AgentFileManager.fromDirectory(tempRoot)
        val outsideDir = createTempDir(prefix = "outside-agent-file-manager-test")
        try {
            File(outsideDir, "secret.md").writeText("top secret")
            Files.createSymbolicLink(File(tempRoot, "link").toPath(), outsideDir.toPath())

            assertFailsWith<SecurityException> {
                manager.readFile("link/secret.md")
            }
        } finally {
            outsideDir.deleteRecursively()
        }
    }

    @Test
    fun `SECURITY md is read only`() {
        val manager = AgentFileManager.fromDirectory(tempRoot)

        assertFailsWith<SecurityException> {
            manager.writeFile("SECURITY.md", "overwrite denied")
        }
    }

    @Test
    fun `dailyMemoryPath generates expected relative file path`() {
        val manager = AgentFileManager.fromDirectory(tempRoot)
        val path = manager.dailyMemoryPath(LocalDate.of(2026, 2, 12))

        assertEquals("memory/2026-02-12.md", path)
    }

    // ========== File Size Limit Tests (#313) ==========

    @Test
    fun `readFile rejects files exceeding max size`() {
        val manager = AgentFileManager.fromDirectory(tempRoot)
        // Write a file that exceeds MAX_READ_SIZE_BYTES directly (bypass writeFile limit)
        val file = File(tempRoot, "huge.md")
        file.writeText("x".repeat((AgentFileManager.MAX_READ_SIZE_BYTES + 1).toInt()))

        val error = assertFailsWith<IllegalArgumentException> {
            manager.readFile("huge.md")
        }
        assertTrue(error.message!!.contains("File too large"))
    }

    @Test
    fun `writeFile rejects content exceeding max size`() {
        val manager = AgentFileManager.fromDirectory(tempRoot)
        val oversized = "x".repeat((AgentFileManager.MAX_WRITE_SIZE_BYTES + 1).toInt())

        val error = assertFailsWith<IllegalArgumentException> {
            manager.writeFile("huge.md", oversized)
        }
        assertTrue(error.message!!.contains("Content too large"))
    }

    @Test
    fun `readFile accepts files at max size`() {
        val manager = AgentFileManager.fromDirectory(tempRoot)
        val content = "x".repeat(AgentFileManager.MAX_READ_SIZE_BYTES.toInt())
        val file = File(tempRoot, "maxsize.md")
        file.writeText(content)

        val result = manager.readFile("maxsize.md")
        assertEquals(content, result)
    }

    @Test
    fun `writeFile accepts content at max size`() {
        val manager = AgentFileManager.fromDirectory(tempRoot)
        val content = "x".repeat(AgentFileManager.MAX_WRITE_SIZE_BYTES.toInt())

        manager.writeFile("maxsize.md", content)
        assertEquals(content, manager.readFile("maxsize.md"))
    }

    // -- onPromptFileChanged callback tests --

    @Test
    fun `writeFile triggers onPromptFileChanged for SOUL_MD`() {
        val manager = AgentFileManager.fromDirectory(tempRoot)
        val triggered = mutableListOf<String>()
        manager.onPromptFileChanged = { triggered.add(it) }

        manager.writeFile("SOUL.md", "# My soul")

        assertEquals(listOf("SOUL.md"), triggered)
    }

    @Test
    fun `writeFile triggers onPromptFileChanged for IDENTITY_MD`() {
        val manager = AgentFileManager.fromDirectory(tempRoot)
        val triggered = mutableListOf<String>()
        manager.onPromptFileChanged = { triggered.add(it) }

        manager.writeFile("IDENTITY.md", "# Identity")

        assertEquals(listOf("IDENTITY.md"), triggered)
    }

    @Test
    fun `writeFile triggers onPromptFileChanged for USER_MD`() {
        val manager = AgentFileManager.fromDirectory(tempRoot)
        val triggered = mutableListOf<String>()
        manager.onPromptFileChanged = { triggered.add(it) }

        manager.writeFile("USER.md", "# User info")

        assertEquals(listOf("USER.md"), triggered)
    }

    @Test
    fun `writeFile does NOT trigger onPromptFileChanged for non-prompt files`() {
        val manager = AgentFileManager.fromDirectory(tempRoot)
        val triggered = mutableListOf<String>()
        manager.onPromptFileChanged = { triggered.add(it) }

        manager.writeFile("AGENTS.md", "# Agents")
        manager.writeFile("notes.md", "some notes")
        manager.writeFile("memory/2026-01-01.md", "daily log")

        assertTrue(triggered.isEmpty(), "Non-prompt files should not trigger callback")
    }

    // ── Knowledge tests ──

    @Test
    fun `knowledgePathForPackage returns correct path`() {
        val manager = AgentFileManager.fromDirectory(tempRoot)
        assertEquals(
            "knowledge/app-com.google.android.apps.messaging.md",
            manager.knowledgePathForPackage("com.google.android.apps.messaging")
        )
    }

    @Test
    fun `knowledgePathForPackage sanitizes special characters`() {
        val manager = AgentFileManager.fromDirectory(tempRoot)
        val path = manager.knowledgePathForPackage("com.evil/../escape")
        assertFalse(path.contains(".."), "Should sanitize path traversal: $path")
    }

    @Test
    fun `knowledgePathForPackage rejects package names that sanitize to empty`() {
        val manager = AgentFileManager.fromDirectory(tempRoot)
        assertFailsWith<IllegalArgumentException> {
            manager.knowledgePathForPackage("!!!")
        }
    }

    @Test
    fun `readKnowledge returns null when no file exists`() {
        val manager = AgentFileManager.fromDirectory(tempRoot)
        assertNull(manager.readKnowledge("com.nonexistent.app"))
    }

    @Test
    fun `writeKnowledge creates new file with header and entry`() {
        val manager = AgentFileManager.fromDirectory(tempRoot)
        val result = manager.writeKnowledge("com.test.app", "Tap by text works better", "navigation")

        assertTrue(result.contains("# com.test.app"))
        assertTrue(result.contains("## Navigation"))
        assertTrue(result.contains("[confirmed:1] Tap by text works better"))
        assertTrue(result.contains("Last Updated:"))

        // Verify persisted
        val read = manager.readKnowledge("com.test.app")
        assertNotNull(read)
        assertTrue(read!!.contains("[confirmed:1] Tap by text works better"))
    }

    @Test
    fun `writeKnowledge appends to existing category`() {
        val manager = AgentFileManager.fromDirectory(tempRoot)
        manager.writeKnowledge("com.test.app", "First pattern", "navigation")
        manager.writeKnowledge("com.test.app", "Second pattern", "navigation")

        val content = manager.readKnowledge("com.test.app")!!
        assertTrue(content.contains("[confirmed:1] First pattern"))
        assertTrue(content.contains("[confirmed:1] Second pattern"))
        // Only one Navigation header
        assertEquals(1, content.lines().count { it.trim() == "## Navigation" })
    }

    @Test
    fun `writeKnowledge creates separate category sections`() {
        val manager = AgentFileManager.fromDirectory(tempRoot)
        manager.writeKnowledge("com.test.app", "Nav pattern", "navigation")
        manager.writeKnowledge("com.test.app", "Fail pattern", "failure")

        val content = manager.readKnowledge("com.test.app")!!
        assertTrue(content.contains("## Navigation"))
        assertTrue(content.contains("## Failure"))
        assertTrue(content.contains("[confirmed:1] Nav pattern"))
        assertTrue(content.contains("[confirmed:1] Fail pattern"))
    }

    @Test
    fun `writeKnowledge throws when file would exceed size limit`() {
        val manager = AgentFileManager.fromDirectory(tempRoot)
        // Fill with a large pattern to approach the limit
        val bigPattern = "x".repeat(1800)
        manager.writeKnowledge("com.test.app", bigPattern, "navigation")

        // Second write should exceed 2KB
        assertFailsWith<IllegalArgumentException> {
            manager.writeKnowledge("com.test.app", bigPattern, "failure")
        }
    }

    @Test
    fun `listKnowledgePackages returns empty when no knowledge exists`() {
        val manager = AgentFileManager.fromDirectory(tempRoot)
        assertTrue(manager.listKnowledgePackages().isEmpty())
    }

    @Test
    fun `listKnowledgePackages returns packages with knowledge files`() {
        val manager = AgentFileManager.fromDirectory(tempRoot)
        manager.writeKnowledge("com.app.one", "Pattern 1")
        manager.writeKnowledge("com.app.two", "Pattern 2")

        val packages = manager.listKnowledgePackages()
        assertEquals(listOf("com.app.one", "com.app.two"), packages)
    }
}
