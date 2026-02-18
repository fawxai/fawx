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

        assertTrue(soul.contains("# SOUL"), "Should have SOUL header")
        assertTrue(soul.contains("chill but sharp"), "Should contain agent vibe")
        assertTrue(soul.contains("casual and direct"), "Should contain relationship style")
        assertTrue(soul.contains("Be genuinely helpful"), "Should contain personality guidance")

        // IDENTITY.md should also be written
        val identity = manager.readFile(AgentFileManager.IDENTITY_FILE)
        assertTrue(identity.contains("Zest"), "Identity should contain agent name")
        assertTrue(identity.contains("citrus spirit"), "Identity should contain agent nature")
        assertTrue(identity.contains("🍋"), "Identity should contain emoji")

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

        assertTrue(prompt.contains("Zest"), "Prompt should contain agent name")
        assertTrue(prompt.contains("Joe"), "Prompt should contain user name")
        assertTrue(prompt.contains("## Strategy"), "Prompt should contain phone agent strategy")
    }

    @Test
    fun `systemPromptForStartup falls back before onboarding files exist`() {
        val manager = AgentFileManager.fromDirectory(tempRoot)

        val prompt = OnboardingPersistence.systemPromptForStartup(manager)

        // Always uses composed prompt; with no identity files, falls back to hardcoded SECTION_IDENTITY
        assertTrue(prompt.contains("You are Citros"), "Should contain default identity")
        assertTrue(prompt.contains("## Strategy"), "Should contain strategy section")
    }

    @Test
    fun `buildSoulMarkdown produces valid markdown structure`() {
        val profile = OnboardingTestFixtures.sampleProfile()
        val md = OnboardingPersistence.buildSoulMarkdown(profile)

        // Starts with H1 heading
        assertTrue(md.startsWith("# SOUL"), "Should start with H1 heading")
        // Contains key personality sections
        assertTrue(md.contains("## Personality"), "Should have Personality section")
        assertTrue(md.contains("## Core Truths"), "Should have Core Truths section")
        assertTrue(md.contains("## Boundaries"), "Should have Boundaries section")
        // Contains the agent vibe
        assertTrue(md.contains("chill but sharp"), "Should contain the agent vibe")
        // Contains personality guidance
        assertTrue(md.contains("Be genuinely helpful"), "Should contain personality guidance")
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
