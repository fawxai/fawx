package ai.citros.chat

import ai.citros.core.AgentFileManager
import ai.citros.core.PhoneAgentPrompts
import org.junit.After
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class OnboardingPersistenceTest {
    private val tempRoot = createTempDir(prefix = "onboarding-persistence-test")

    @After
    fun tearDown() {
        tempRoot.deleteRecursively()
    }

    @Test
    fun `persistIdentityProfile writes SOUL and USER markdown`() {
        val manager = AgentFileManager.fromDirectory(tempRoot)
        val profile = OnboardingTestFixtures.sampleProfile()

        OnboardingPersistence.persistIdentityProfile(manager, profile)

        val soul = manager.readFile(AgentFileManager.SOUL_FILE)
        val user = manager.readFile(AgentFileManager.USER_FILE)

        assertTrue(soul.contains("# SOUL"))
        assertTrue(soul.contains("- Name: Zest"))
        assertTrue(soul.contains("- Nature: citrus spirit"))
        assertTrue(soul.contains("- Vibe: chill but sharp"))
        assertTrue(soul.contains("- Emoji: 🍋"))
        assertTrue(soul.contains("- Style: casual and direct"))

        assertTrue(user.contains("# USER"))
        assertTrue(user.contains("- Name: Joe"))
        assertTrue(user.contains("- Address: captain"))
        assertTrue(user.contains("- Relationship style: casual and direct"))
        assertTrue(user.contains("- Boundaries: ask before sending messages"))
        assertTrue(user.contains("- Context: prefers concise updates"))
    }

    @Test
    fun `systemPromptForStartup includes persisted identity content`() {
        val manager = AgentFileManager.fromDirectory(tempRoot)
        OnboardingPersistence.persistIdentityProfile(manager, OnboardingTestFixtures.sampleProfile())

        val prompt = OnboardingPersistence.systemPromptForStartup(manager)

        assertTrue(prompt.contains("## SOUL.md"))
        assertTrue(prompt.contains("## USER.md"))
        assertTrue(prompt.contains("Zest"))
        assertTrue(prompt.contains("Joe"))
    }

    @Test
    fun `systemPromptForStartup falls back before onboarding files exist`() {
        val manager = AgentFileManager.fromDirectory(tempRoot)

        val prompt = OnboardingPersistence.systemPromptForStartup(manager)

        // Falls back to the default built system prompt (contains identity + strategy)
        assertTrue(prompt.contains("You are Citros"), "Should fall back to default system prompt")
        assertTrue(prompt.contains("## Strategy"), "Should contain strategy section")
    }

    @Test
    fun `buildSoulMarkdown produces valid markdown structure`() {
        val profile = OnboardingTestFixtures.sampleProfile()
        val md = OnboardingPersistence.buildSoulMarkdown(profile)

        // Starts with H1 heading
        assertTrue(md.startsWith("# SOUL"), "Should start with H1 heading")
        // Contains H2 sections
        assertTrue(md.contains("## Identity"), "Should have Identity section")
        assertTrue(md.contains("## Relationship"), "Should have Relationship section")
        // All list items are valid markdown bullets
        val bulletLines = md.lines().filter { it.trimStart().startsWith("- ") }
        assertTrue(bulletLines.size >= 5, "Should have at least 5 bullet items, got ${bulletLines.size}")
        bulletLines.forEach { line ->
            assertTrue(line.matches(Regex("^- .+: .+$")),
                "Bullet should be '- Key: Value' format, got: '$line'")
        }
        // No blank value fields
        bulletLines.forEach { line ->
            val value = line.substringAfter(": ")
            assertTrue(value.isNotBlank(), "Value should not be blank in: '$line'")
        }
    }

    @Test
    fun `buildUserMarkdown produces valid markdown structure`() {
        val profile = OnboardingTestFixtures.sampleProfile()
        val md = OnboardingPersistence.buildUserMarkdown(profile)

        assertTrue(md.startsWith("# USER"), "Should start with H1 heading")
        assertTrue(md.contains("## Core"), "Should have Core section")
        assertTrue(md.contains("## Preferences"), "Should have Preferences section")
        val bulletLines = md.lines().filter { it.trimStart().startsWith("- ") }
        assertTrue(bulletLines.size >= 5, "Should have at least 5 bullet items, got ${bulletLines.size}")
        bulletLines.forEach { line ->
            assertTrue(line.matches(Regex("^- .+: .+$")),
                "Bullet should be '- Key: Value' format, got: '$line'")
        }
    }

    @Test
    fun `generated markdown has no trailing whitespace or empty lines between sections`() {
        val profile = OnboardingTestFixtures.sampleProfile()
        val soul = OnboardingPersistence.buildSoulMarkdown(profile)
        val user = OnboardingPersistence.buildUserMarkdown(profile)

        listOf("SOUL" to soul, "USER" to user).forEach { (name, md) ->
            // No trailing whitespace on any line
            md.lines().forEachIndexed { i, line ->
                assertEquals(line.trimEnd(), line,
                    "$name line $i has trailing whitespace")
            }
            // No triple+ newlines (excessive blank lines)
            assertFalse(md.contains("\n\n\n"),
                "$name has excessive blank lines")
        }
    }
}
