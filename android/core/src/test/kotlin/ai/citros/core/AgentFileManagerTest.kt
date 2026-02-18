package ai.citros.core

import org.junit.After
import org.junit.Test
import java.io.File
import java.nio.file.Files
import java.time.LocalDate
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
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
}
