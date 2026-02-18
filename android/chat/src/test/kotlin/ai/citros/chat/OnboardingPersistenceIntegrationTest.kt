package ai.citros.chat

import ai.citros.core.AgentFileManager
import ai.citros.core.ChatResponse
import ai.citros.core.Conversation
import ai.citros.core.Message
import ai.citros.core.PhoneAgentApi
import ai.citros.core.PhoneAgentPrompts
import ai.citros.core.Provider
import ai.citros.core.ProviderClient
import ai.citros.core.Tool
import android.content.Context
import kotlinx.coroutines.test.runTest
import org.junit.After
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

/**
 * Integration tests for onboarding identity persistence and startup prompt wiring.
 *
 * Unlike [OnboardingPersistenceTest] (unit-level markdown + prompt builder checks),
 * these tests validate the real round-trip through Android storage:
 * 1) onboarding completion state in SharedPreferences
 * 2) identity files persisted via [AgentFileManager]
 * 3) fresh manager instance after simulated app reload
 * 4) prompt actually sent into chat via [PhoneAgentApi]
 */
@RunWith(RobolectricTestRunner::class)
class OnboardingPersistenceIntegrationTest {

    private lateinit var context: Context

    @Before
    fun setUp() {
        context = RuntimeEnvironment.getApplication()
        context.getSharedPreferences(ONBOARDING_PREFS, Context.MODE_PRIVATE).edit().clear().commit()

        val agentDir = context.filesDir.resolve("agent")
        agentDir.deleteRecursively()
        check(!agentDir.exists()) { "Agent directory should be deleted" }

        // Recreate defaults so each test starts from clean app state.
        AgentFileManager.fromContext(context)
        check(agentDir.exists()) { "Agent directory should exist after initialization" }
    }

    @After
    fun tearDown() {
        context.getSharedPreferences(ONBOARDING_PREFS, Context.MODE_PRIVATE).edit().clear().commit()

        val agentDir = context.filesDir.resolve("agent")
        agentDir.deleteRecursively()
        check(!agentDir.exists()) { "Agent directory should be removed during teardown" }
    }

    @Test
    fun `onboarding identity persists and survives app reload`() = runTest {
        val profile = OnboardingTestFixtures.sampleProfile()

        val onboardingManager = AgentFileManager.fromContext(context)
        OnboardingPersistence.persistIdentityProfile(onboardingManager, profile)
        context.getSharedPreferences(ONBOARDING_PREFS, Context.MODE_PRIVATE)
            .edit()
            .putBoolean(PREF_ONBOARDING_COMPLETE, true)
            .commit()

        // Simulate process/app reload by constructing fresh manager from persisted files.
        val reloadedManager = AgentFileManager.fromContext(context)
        val persistedSoul = reloadedManager.readFile(AgentFileManager.SOUL_FILE)
        val persistedUser = reloadedManager.readFile(AgentFileManager.USER_FILE)
        val startupPrompt = OnboardingPersistence.systemPromptForStartup(reloadedManager)

        val prefs = context.getSharedPreferences(ONBOARDING_PREFS, Context.MODE_PRIVATE)
        assertTrue(
            prefs.getBoolean(PREF_ONBOARDING_COMPLETE, false),
            "Onboarding completion flag should persist"
        )
        assertFalse(shouldShowOnboarding(context))

        val client = CapturingProviderClient()
        client.reset()
        val agent = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            agentFileManager = reloadedManager
        )
        agent.phoneControlOverride = true

        val response = agent.sendMessage(
            "open settings",
            screenContent = null // Screen context is irrelevant for identity prompt assertions.
        )

        // Agent name is in IDENTITY.md, not SOUL.md; SOUL has personality/vibe
        assertTrue(persistedSoul.contains(profile.agentVibe), "SOUL should contain agent vibe")
        val persistedIdentity = reloadedManager.readFile(AgentFileManager.IDENTITY_FILE)
        assertTrue(persistedIdentity.contains(profile.agentName), "IDENTITY should contain agent name")
        assertTrue(persistedUser.contains(profile.userName), "USER should contain user name")
        assertTrue(startupPrompt.contains(profile.agentName), "Startup prompt should contain agent name")
        assertTrue(startupPrompt.contains(profile.userName), "Startup prompt should contain user name")
        assertTrue(startupPrompt.contains("## Strategy"), "Startup prompt should contain strategy section")

        assertEquals(1, client.chatWithToolsCallCount)
        val sentPrompt = client.lastSystemPrompt
        assertNotNull(sentPrompt)
        assertTrue(sentPrompt.contains(profile.agentName), "Sent prompt should contain agent name")
        assertTrue(sentPrompt.contains(profile.userName), "Sent prompt should contain user name")
        assertTrue(sentPrompt.contains("## Strategy"), "Sent prompt should contain strategy section")

        assertEquals("Done", response.text)
        assertEquals("end_turn", response.stopReason)
    }

    @Test
    fun `systemPromptForStartup falls back when user file missing after reload`() = runTest {
        val profile = OnboardingTestFixtures.sampleProfile()
        val manager = AgentFileManager.fromContext(context)

        OnboardingPersistence.persistIdentityProfile(manager, profile)

        context.filesDir.resolve("agent/${AgentFileManager.USER_FILE}").delete()

        val reloadedManager = AgentFileManager.fromContext(context)
        val prompt = OnboardingPersistence.systemPromptForStartup(reloadedManager)

        // With user file missing, composed prompt still has identity from IDENTITY.md
        assertTrue(prompt.contains(profile.agentName), "Should contain agent name from IDENTITY.md")
        assertTrue(prompt.contains("## Strategy"), "Should contain strategy section")
    }

    @Test
    fun `systemPromptForStartup falls back when soul file missing after reload`() = runTest {
        val profile = OnboardingTestFixtures.sampleProfile()
        val manager = AgentFileManager.fromContext(context)

        OnboardingPersistence.persistIdentityProfile(manager, profile)

        context.filesDir.resolve("agent/${AgentFileManager.SOUL_FILE}").delete()

        val reloadedManager = AgentFileManager.fromContext(context)
        val prompt = OnboardingPersistence.systemPromptForStartup(reloadedManager)

        // With soul file missing, composed prompt still has identity from IDENTITY.md
        assertTrue(prompt.contains(profile.agentName), "Should contain agent name from IDENTITY.md")
        assertTrue(prompt.contains("## Strategy"), "Should contain strategy section")
    }

    private class CapturingProviderClient(
        override val provider: Provider = Provider.ANTHROPIC // Stub; not used in prompt assertions
    ) : ProviderClient {
        var lastSystemPrompt: String? = null
            private set

        var chatWithToolsCallCount: Int = 0
            private set

        fun reset() {
            lastSystemPrompt = null
            chatWithToolsCallCount = 0
        }

        override suspend fun chat(conversation: Conversation): Result<String> {
            return Result.success("unused")
        }

        override suspend fun chatWithTools(
            messages: List<Message>,
            systemPrompt: String?,
            tools: List<Tool>,
            tokenLimit: Int?
        ): Result<ChatResponse> {
            chatWithToolsCallCount++
            lastSystemPrompt = systemPrompt
            return Result.success(
                ChatResponse(
                    text = "Done",
                    toolCalls = emptyList(),
                    stopReason = "end_turn"
                )
            )
        }

        override suspend fun describeImage(base64Image: String, prompt: String, maxTokens: Int): Result<String> {
            return Result.success("unused")
        }
    }
}
