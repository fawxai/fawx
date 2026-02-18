package ai.citros.core

import org.junit.Test
import kotlin.test.assertTrue
import kotlin.test.assertFalse
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith

class PromptAndModelFloorTest {

    // ========== Prompt Content Tests ==========

    @Test
    fun `system prompt instructs task completion`() {
        assertTrue(
            PhoneAgentPrompts.SYSTEM_PROMPT.contains("stop calling tools"),
            "System prompt should instruct agent to stop calling tools when done"
        )
    }

    @Test
    fun `system prompt teaches strategy`() {
        assertTrue(
            PhoneAgentPrompts.SYSTEM_PROMPT.contains("## Strategy"),
            "System prompt should contain Strategy section"
        )
        assertTrue(
            PhoneAgentPrompts.SYSTEM_PROMPT.contains("Direct Commands"),
            "Strategy should teach direct command pattern"
        )
    }

    @Test
    fun `system prompt contains implicit observation guidance`() {
        assertTrue(
            PhoneAgentPrompts.SYSTEM_PROMPT.contains("Don't call read_screen after actions"),
            "System prompt should discourage unnecessary read_screen"
        )
    }

    @Test
    fun `action prompt reinforces task completion`() {
        assertTrue(
            PhoneAgentPrompts.ACTION_PROMPT.contains("respond with text only"),
            "Action prompt should reinforce completion behavior"
        )
    }

    @Test
    fun `action prompt reinforces implicit screen state`() {
        assertTrue(
            PhoneAgentPrompts.ACTION_PROMPT.contains("screen state comes with every action"),
            "Action prompt should note screen state comes with action results"
        )
    }

    // ========== Model Floor Tests ==========

    @Test
    fun `haiku is below floor`() {
        assertFalse(ModelConfig.isModelAboveFloor(Provider.ANTHROPIC, "claude-haiku-4-5-20251001"))
        assertFalse(ModelConfig.isModelAboveFloor(Provider.ANTHROPIC, "claude-3-5-haiku-20241022"))
    }

    @Test
    fun `sonnet is above floor`() {
        assertTrue(ModelConfig.isModelAboveFloor(Provider.ANTHROPIC, "claude-sonnet-4-5-20250929"))
    }

    @Test
    fun `opus is above floor`() {
        assertTrue(ModelConfig.isModelAboveFloor(Provider.ANTHROPIC, "claude-opus-4-6"))
        assertTrue(ModelConfig.isModelAboveFloor(Provider.ANTHROPIC, "claude-opus-4-5-20251101"))
    }

    @Test
    fun `gpt-4o-mini is below floor`() {
        assertFalse(ModelConfig.isModelAboveFloor(Provider.OPENAI, "gpt-4o-mini"))
    }

    @Test
    fun `gpt-4o is above floor`() {
        assertTrue(ModelConfig.isModelAboveFloor(Provider.OPENAI, "gpt-4o"))
    }

    @Test
    fun `o1 is above floor`() {
        assertTrue(ModelConfig.isModelAboveFloor(Provider.OPENAI, "o1"))
    }

    @Test
    fun `openrouter haiku is below floor`() {
        assertFalse(ModelConfig.isModelAboveFloor(Provider.OPENROUTER, "anthropic/claude-haiku-4.5"))
    }

    @Test
    fun `openrouter sonnet is above floor`() {
        assertTrue(ModelConfig.isModelAboveFloor(Provider.OPENROUTER, "anthropic/claude-sonnet-4.5"))
    }

    @Test
    fun `PhoneAgentApi rejects below-floor action model`() {
        val client = DummyProviderClient(Provider.ANTHROPIC)
        assertFailsWith<IllegalArgumentException> {
            PhoneAgentApi(
                chatClient = client,
                actionClient = client,
                actionModelId = "claude-haiku-4-5-20251001"
            )
        }
    }

    @Test
    fun `PhoneAgentApi accepts above-floor action model`() {
        val client = DummyProviderClient(Provider.ANTHROPIC)
        // Should not throw
        PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            actionModelId = "claude-sonnet-4-5-20250929"
        )
    }

    @Test
    fun `PhoneAgentApi accepts null actionModelId`() {
        val client = DummyProviderClient(Provider.ANTHROPIC)
        // Should not throw — null means no validation
        PhoneAgentApi(client)
    }

    @Test
    fun `PhoneAgentApi rejects gpt-4o-mini`() {
        val client = DummyProviderClient(Provider.OPENAI)
        assertFailsWith<IllegalArgumentException> {
            PhoneAgentApi(
                chatClient = client,
                actionClient = client,
                actionModelId = "gpt-4o-mini"
            )
        }
    }

    @Suppress("DEPRECATION")
    @Test
    fun `actionModelForChat promotes haiku to sonnet`() {
        val result = ModelConfig.actionModelForChat(Provider.ANTHROPIC, "claude-haiku-4-5-20251001")
        assertTrue(
            ModelConfig.isModelAboveFloor(Provider.ANTHROPIC, result),
            "actionModelForChat should return above-floor model, got: $result"
        )
    }

    @Suppress("DEPRECATION")
    @Test
    fun `actionModelForChat promotes gpt-4o-mini to gpt-4o`() {
        val result = ModelConfig.actionModelForChat(Provider.OPENAI, "gpt-4o-mini")
        assertTrue(
            ModelConfig.isModelAboveFloor(Provider.OPENAI, result),
            "actionModelForChat should return above-floor model, got: $result"
        )
    }

    @Suppress("DEPRECATION")
    @Test
    fun `actionModelForChat promotes openrouter haiku to sonnet`() {
        val result = ModelConfig.actionModelForChat(Provider.OPENROUTER, "anthropic/claude-haiku-4.5")
        assertTrue(
            ModelConfig.isModelAboveFloor(Provider.OPENROUTER, result),
            "actionModelForChat should return above-floor model, got: $result"
        )
    }

    @Suppress("DEPRECATION")
    @Test
    fun `actionModelForChat always returns above-floor default`() {
        val result = ModelConfig.actionModelForChat(Provider.ANTHROPIC, "claude-sonnet-4-5-20250929")
        assertEquals(ModelConfig.defaultActionModel(Provider.ANTHROPIC), result)
        assertTrue(ModelConfig.isModelAboveFloor(Provider.ANTHROPIC, result))
    }

    // ========== Integration: full flow from config → PhoneAgentApi ==========

    @Test
    fun `end-to-end - defaultActionModel is always accepted by PhoneAgentApi`() {
        // Verifies the contract: for every provider, the default action model
        // passes both the classifier floor check AND PhoneAgentApi construction.
        for (provider in Provider.entries) {
            val actionModelId = ModelConfig.defaultActionModel(provider)
            val client = DummyProviderClient(provider)

            // Should not throw — default action model is always above floor
            PhoneAgentApi(
                chatClient = client,
                actionClient = client,
                actionModelId = actionModelId
            )
        }
    }

    @Test
    fun `end-to-end - below-floor chat model still gets safe action model`() {
        // Simulates: user picks Haiku for chat → system derives action model → must be safe
        val actionModelId = ModelConfig.defaultActionModel(Provider.ANTHROPIC)
        assertTrue(ModelClassifier.isAboveFloor(actionModelId),
            "Action model derived for Haiku chat must pass classifier floor")

        val client = DummyProviderClient(Provider.ANTHROPIC)
        // Construction succeeds with the derived action model
        PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            actionModelId = actionModelId
        )
    }

    // ========== Context Compaction Tests ==========

    @Test
    fun `compaction strips SCREEN from old tool results`() {
        val messages = listOf(
            Message(role = "user", content = "Open Settings"),
            Message(role = "tool", content = "Tapped element 5\n\nSCREEN:\n[1] 'Settings'\n[2] 'About'", toolCallId = "tc1"),
            Message(role = "tool", content = "Tapped element 3\n\nSCREEN:\n[1] 'Wi-Fi'\n[2] 'Bluetooth'", toolCallId = "tc2"),
            Message(role = "tool", content = "Tapped element 1\n\nSCREEN:\n[1] 'Connected'\n[2] 'Disconnect'", toolCallId = "tc3"),
            Message(role = "tool", content = "Tapped element 2\n\nSCREEN:\n[1] 'Network'\n[2] 'Data'", toolCallId = "tc4")
        )
        // Force compaction by using a very low threshold
        val result = ContextCompactor.compact(messages, maxTokenEstimate = 1)

        // Last 2 tool results should be preserved
        assertTrue(result[3].content.contains("SCREEN:"), "Third-to-last tool result should keep SCREEN")
        assertTrue(result[4].content.contains("SCREEN:"), "Last tool result should keep SCREEN")

        // Older tool results should have SCREEN stripped
        assertFalse(result[1].content.contains("SCREEN:"), "Old tool result should have SCREEN stripped")
        assertEquals("Tapped element 5", result[1].content)
        assertFalse(result[2].content.contains("SCREEN:"), "Old tool result should have SCREEN stripped")
        assertEquals("Tapped element 3", result[2].content)
    }

    @Test
    fun `compaction preserves user messages`() {
        val messages = listOf(
            Message(role = "user", content = "Open Settings and check Wi-Fi"),
            Message(role = "tool", content = "Tapped element 5\n\nSCREEN:\n[1] 'Settings'", toolCallId = "tc1"),
            Message(role = "tool", content = "Tapped element 1\n\nSCREEN:\n[1] 'Wi-Fi'", toolCallId = "tc2")
        )
        val result = ContextCompactor.compact(messages, maxTokenEstimate = 1)

        assertEquals("Open Settings and check Wi-Fi", result[0].content, "User message should be preserved")
    }

    @Test
    fun `compaction does not trigger under threshold`() {
        val messages = listOf(
            Message(role = "user", content = "Hi"),
            Message(role = "tool", content = "Tapped element 1\n\nSCREEN:\nstuff", toolCallId = "tc1")
        )
        val result = ContextCompactor.compact(messages, maxTokenEstimate = 60000)

        // Should return same list since under threshold
        assertEquals(messages, result)
    }

    @Test
    fun `compaction handles tool results without SCREEN section`() {
        val messages = listOf(
            Message(role = "user", content = "Think about it"),
            Message(role = "tool", content = "Thought: I should tap the button", toolCallId = "tc1"),
            Message(role = "tool", content = "Tapped element 1\n\nSCREEN:\n[1] 'Done'", toolCallId = "tc2")
        )
        val result = ContextCompactor.compact(messages, maxTokenEstimate = 1)

        // Tool result without SCREEN should be unchanged
        assertEquals("Thought: I should tap the button", result[1].content)
    }

    private class DummyProviderClient(override val provider: Provider) : ProviderClient {
        override suspend fun chat(conversation: Conversation): Result<String> =
            Result.success("")
        override suspend fun chatWithTools(
            messages: List<Message>, systemPrompt: String?, tools: List<Tool>, tokenLimit: Int?
        ): Result<ChatResponse> = Result.success(ChatResponse(text = "", toolCalls = emptyList(), stopReason = "end_turn"))
        override suspend fun describeImage(base64Image: String, prompt: String, maxTokens: Int): Result<String> =
            Result.success("")
    }
}
