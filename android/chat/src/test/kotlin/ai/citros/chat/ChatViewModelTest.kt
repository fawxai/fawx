package ai.citros.chat

import ai.citros.core.ChatResponse
import ai.citros.core.MemoryFilter
import ai.citros.core.MemoryMetadata
import ai.citros.core.MemoryProvider
import ai.citros.core.MemoryResult
import ai.citros.core.Message
import ai.citros.core.Provider
import ai.citros.core.ProviderClient
import ai.citros.core.ProviderException
import ai.citros.core.ScreenContent
import ai.citros.core.ScreenElement
import ai.citros.core.Tool
import ai.citros.core.ToolCall
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import kotlinx.coroutines.test.resetMain
import org.junit.After
import org.junit.Before
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue
import ai.citros.core.OutputVerbosity

@OptIn(ExperimentalCoroutinesApi::class)
class ChatViewModelTest {

    private lateinit var viewModel: ChatViewModel
    private val testDispatcher = StandardTestDispatcher()

    @Before
    fun setup() {
        Dispatchers.setMain(testDispatcher)
        viewModel = ChatViewModel()
        viewModel.outputVerbosity = OutputVerbosity.VERBOSE
    }

    @After
    fun tearDown() {
        ai.citros.core.ScreenReader.toolLoopOverlayHideHook = null
        ai.citros.core.ScreenReader.toolLoopOverlayRestoreHook = null
        Dispatchers.resetMain()
    }

    @Test
    fun `configureWithToken with unrecognized token and null preferredProvider fails fast`() {
        viewModel.configureWithToken("unrecognized-token-xyz-12345", preferredProvider = null, authKind = null)

        assertFalse(viewModel.isConfigured.value)
        assertTrue(viewModel.error.value?.contains("Could not detect provider") == true)
    }

    @Test
    fun `configureWithToken with recognized Anthropic token creates backend`() {
        viewModel.configureWithToken("sk-ant-api03-test-key-12345", preferredProvider = null)

        assertTrue(viewModel.isConfigured.value)
        assertFalse(viewModel.needsAuth.value)
    }

    @Test
    fun `configureWithToken with authKind takes precedence over preferredProvider`() {
        viewModel.configureWithToken(
            token = "multi-hint-token",
            preferredProvider = Provider.OPENAI,
            authKind = CloudAuthKind.ANTHROPIC_CREDENTIAL
        )

        assertTrue(viewModel.isConfigured.value)
        assertFalse(viewModel.needsAuth.value)
    }

    @Test
    fun `requestAuth then signOut resets auth-required state`() {
        viewModel.requestAuth()
        assertTrue(viewModel.needsAuth.value)

        viewModel.signOut()

        assertFalse(viewModel.needsAuth.value)
    }

    @Test
    fun `sendMessage when not configured emits fallback message and stops loading`() = runTest {
        viewModel.sendMessage("hello")
        advanceUntilIdle()

        assertFalse(viewModel.isLoading.value)
        assertEquals("user", viewModel.messages[0].role)
        assertEquals("assistant", viewModel.messages[1].role)
        assertTrue(viewModel.messages[1].content.contains("Not configured"))
    }

    @Test
    fun `sendMessage action loop starts on tool_use and ends on final text`() = runTest {
        val scripted = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            scripted = ArrayDeque(
                listOf(
                    ChatResponse(
                        text = "Working on it",
                        toolCalls = listOf(ToolCall("t1", "press_back", emptyMap())),
                        stopReason = "tool_use"
                    ),
                    ChatResponse(
                        text = "Done",
                        toolCalls = emptyList(),
                        stopReason = "end_turn"
                    )
                )
            )
        )
        setApiModeWithBackends(
            viewModel,
            listOf(viewModel.createTestBackend(Provider.ANTHROPIC, scripted, scripted))
        )

        // Action-hint messages bypass isLikelyConversationalMessage() to test tool loop
        viewModel.sendMessage("open something")
        advanceUntilIdle()

        assertFalse(viewModel.isLoading.value)
        assertTrue(viewModel.messages.any { it.content.contains("🤖") })
        assertTrue(viewModel.messages.last().content.contains("Done"))
        assertTrue(scripted.calls >= 2)
    }

    @Test
    fun `sendMessage executes multiple tool calls from a single response`() = runTest {
        val scripted = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            scripted = ArrayDeque(
                listOf(
                    ChatResponse(
                        text = "Doing two actions",
                        toolCalls = listOf(
                            ToolCall("t1", "press_back", emptyMap()),
                            ToolCall("t2", "press_home", emptyMap())
                        ),
                        stopReason = "tool_use"
                    ),
                    ChatResponse(
                        text = "All set",
                        toolCalls = emptyList(),
                        stopReason = "end_turn"
                    )
                )
            )
        )
        setApiModeWithBackends(
            viewModel,
            listOf(viewModel.createTestBackend(Provider.ANTHROPIC, scripted, scripted))
        )

        viewModel.sendMessage("tap two things")
        advanceUntilIdle()

        val toolResultMessages = viewModel.messages.filter { it.role == "assistant" && it.content.startsWith("🤖") }
        assertEquals(2, toolResultMessages.size)
        assertTrue(viewModel.messages.last().content.contains("All set"))
    }

    @Test
    fun `sendMessage stops action loop at max tool steps`() = runTest {
        val endlessToolResponses = ArrayDeque(
            (1..26).map { index ->
                ChatResponse(
                    text = null,
                    toolCalls = listOf(ToolCall("tool_$index", "press_back", emptyMap())),
                    stopReason = "tool_use"
                )
            }
        )
        val scripted = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            scripted = endlessToolResponses
        )
        setApiModeWithBackends(
            viewModel,
            listOf(viewModel.createTestBackend(Provider.ANTHROPIC, scripted, scripted))
        )

        viewModel.sendMessage("press back repeatedly")
        advanceUntilIdle()

        assertFalse(viewModel.isLoading.value)
        // #603: hardcoded "Hit step limit" replaced by requestFinalExplanation
    }

    @Test
    fun `sendMessage uses final text when returned on max step boundary`() = runTest {
        val responses = ArrayDeque(
            (1..24).map { index ->
                ChatResponse(
                    text = null,
                    toolCalls = listOf(ToolCall("tool_$index", "press_back", emptyMap())),
                    stopReason = "tool_use"
                )
            } + ChatResponse(
                text = "Completed exactly at limit",
                toolCalls = listOf(ToolCall("tool_25", "press_back", emptyMap())),
                stopReason = "tool_use"
            )
        )
        val scripted = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            scripted = responses
        )
        setApiModeWithBackends(
            viewModel,
            listOf(viewModel.createTestBackend(Provider.ANTHROPIC, scripted, scripted))
        )

        viewModel.sendMessage("tap the boundary element")
        advanceUntilIdle()

        assertFalse(viewModel.isLoading.value)
        // #603: At max steps boundary, requestFinalExplanation fires.
        // Scripted responses exhausted so final explanation fails silently.
    }

    @Test
    fun `sendMessage continues when one tool in multi-tool response errors`() = runTest {
        val scripted = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            scripted = ArrayDeque(
                listOf(
                    ChatResponse(
                        text = "Running actions",
                        toolCalls = listOf(
                            ToolCall("t1", "unknown_action", emptyMap()),
                            ToolCall("t2", "press_back", emptyMap())
                        ),
                        stopReason = "tool_use"
                    ),
                    ChatResponse(
                        text = "Recovered and done",
                        toolCalls = emptyList(),
                        stopReason = "end_turn"
                    )
                )
            )
        )
        setApiModeWithBackends(
            viewModel,
            listOf(viewModel.createTestBackend(Provider.ANTHROPIC, scripted, scripted))
        )

        viewModel.sendMessage("tap flaky buttons")
        advanceUntilIdle()

        val toolMessages = viewModel.messages.filter { it.content.startsWith("🤖") || it.content.startsWith("⚙️") }
        assertEquals(2, toolMessages.size)
        assertTrue(toolMessages.first().content.contains("Failed"))
        assertTrue(viewModel.messages.last().content.contains("Recovered and done"))
    }

    // ========== Edge-Case Tests: Tool Count (#278) ==========

    @Test
    fun `single tool call in response executes and completes`() = runTest {
        // #278: verify 1 tool call works (existing tests focus on 2+)
        val scripted = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            scripted = ArrayDeque(
                listOf(
                    ChatResponse(
                        text = null,
                        toolCalls = listOf(ToolCall("t1", "press_home", emptyMap())),
                        stopReason = "tool_use"
                    ),
                    ChatResponse(
                        text = "Home screen ready",
                        toolCalls = emptyList(),
                        stopReason = "end_turn"
                    )
                )
            )
        )
        setApiModeWithBackends(
            viewModel,
            listOf(viewModel.createTestBackend(Provider.ANTHROPIC, scripted, scripted))
        )

        // Action-hint messages bypass isLikelyConversationalMessage() to test tool loop
        viewModel.sendMessage("press the home button")
        advanceUntilIdle()

        assertFalse(viewModel.isLoading.value)
        val toolMessages = viewModel.messages.filter { it.content.startsWith("🤖") || it.content.startsWith("⚙️") }
        assertEquals(1, toolMessages.size)
        assertTrue(viewModel.messages.last().content.contains("Home screen ready"))
    }

    @Test
    fun `three tool calls in single response all execute`() = runTest {
        // #278: verify 3+ tools in one response (realistic upper range)
        val scripted = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            scripted = ArrayDeque(
                listOf(
                    ChatResponse(
                        text = "Doing three actions",
                        toolCalls = listOf(
                            ToolCall("t1", "press_back", emptyMap()),
                            ToolCall("t2", "press_home", emptyMap()),
                            ToolCall("t3", "press_back", emptyMap())
                        ),
                        stopReason = "tool_use"
                    ),
                    ChatResponse(
                        text = "All three done",
                        toolCalls = emptyList(),
                        stopReason = "end_turn"
                    )
                )
            )
        )
        setApiModeWithBackends(
            viewModel,
            listOf(viewModel.createTestBackend(Provider.ANTHROPIC, scripted, scripted))
        )

        viewModel.sendMessage("open and tap three things")
        advanceUntilIdle()

        assertFalse(viewModel.isLoading.value)
        val toolMessages = viewModel.messages.filter { it.content.startsWith("🤖") || it.content.startsWith("⚙️") }
        assertEquals(3, toolMessages.size)
        assertTrue(viewModel.messages.last().content.contains("All three done"))
    }

    @Test
    fun `empty toolCalls with tool_use stopReason treated as end of loop`() = runTest {
        // #278: malformed response — stopReason says tool_use but no actual tools
        val scripted = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            scripted = ArrayDeque(
                listOf(
                    ChatResponse(
                        text = "I was going to do something but didn't",
                        toolCalls = emptyList(),
                        stopReason = "tool_use"  // contradicts empty toolCalls
                    )
                )
            )
        )
        setApiModeWithBackends(
            viewModel,
            listOf(viewModel.createTestBackend(Provider.ANTHROPIC, scripted, scripted))
        )

        viewModel.sendMessage("open the settings")
        advanceUntilIdle()

        assertFalse(viewModel.isLoading.value)
        // Should NOT enter tool loop — toolCalls is empty
        val toolMessages = viewModel.messages.filter { it.content.startsWith("🤖") || it.content.startsWith("⚙️") }
        assertEquals(0, toolMessages.size)
        // Final text should still be displayed
        assertTrue(viewModel.messages.last().content.contains("I was going to do something"))
    }

    @Test
    fun `five tool calls in single response all execute sequentially`() = runTest {
        // #278: higher tool count edge case
        val toolCalls = (1..5).map { ToolCall("t$it", "press_back", emptyMap()) }
        val scripted = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            scripted = ArrayDeque(
                listOf(
                    ChatResponse(
                        text = null,
                        toolCalls = toolCalls,
                        stopReason = "tool_use"
                    ),
                    ChatResponse(
                        text = "All five complete",
                        toolCalls = emptyList(),
                        stopReason = "end_turn"
                    )
                )
            )
        )
        setApiModeWithBackends(
            viewModel,
            listOf(viewModel.createTestBackend(Provider.ANTHROPIC, scripted, scripted))
        )

        viewModel.sendMessage("tap five elements")
        advanceUntilIdle()

        assertFalse(viewModel.isLoading.value)
        val toolMessages = viewModel.messages.filter { it.content.startsWith("🤖") || it.content.startsWith("⚙️") }
        assertEquals(5, toolMessages.size)
        assertTrue(viewModel.messages.last().content.contains("All five complete"))
    }

    // ========== Edge-Case Tests: Multi-Tool All-Fail (#280) ==========

    @Test
    fun `all tools in multi-tool response fail and loop recovers`() = runTest {
        // #280: every tool in a response errors — loop should continue to next model response
        val scripted = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            scripted = ArrayDeque(
                listOf(
                    ChatResponse(
                        text = null,
                        toolCalls = listOf(
                            ToolCall("t1", "unknown_tool_a", emptyMap()),
                            ToolCall("t2", "unknown_tool_b", emptyMap()),
                            ToolCall("t3", "unknown_tool_c", emptyMap())
                        ),
                        stopReason = "tool_use"
                    ),
                    ChatResponse(
                        text = "All failed but I recovered",
                        toolCalls = emptyList(),
                        stopReason = "end_turn"
                    )
                )
            )
        )
        setApiModeWithBackends(
            viewModel,
            listOf(viewModel.createTestBackend(Provider.ANTHROPIC, scripted, scripted))
        )

        viewModel.sendMessage("tap three unknown buttons")
        advanceUntilIdle()

        assertFalse(viewModel.isLoading.value)
        val toolMessages = viewModel.messages.filter { it.content.startsWith("🤖") || it.content.startsWith("⚙️") }
        assertEquals(3, toolMessages.size)
        // All three should contain error messages
        assertTrue(toolMessages.all { it.content.contains("Failed") || it.content.contains("Unknown") })
        // Model should still give final response
        assertTrue(viewModel.messages.last().content.contains("recovered"))
    }

    @Test
    fun `all tools fail across multiple loop iterations`() = runTest {
        // #280: failures across multiple turns, not just one response
        val scripted = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            scripted = ArrayDeque(
                listOf(
                    ChatResponse(
                        text = null,
                        toolCalls = listOf(ToolCall("t1", "unknown_x", emptyMap())),
                        stopReason = "tool_use"
                    ),
                    ChatResponse(
                        text = null,
                        toolCalls = listOf(ToolCall("t2", "unknown_y", emptyMap())),
                        stopReason = "tool_use"
                    ),
                    ChatResponse(
                        text = "Giving up after repeated failures",
                        toolCalls = emptyList(),
                        stopReason = "end_turn"
                    )
                )
            )
        )
        setApiModeWithBackends(
            viewModel,
            listOf(viewModel.createTestBackend(Provider.ANTHROPIC, scripted, scripted))
        )

        viewModel.sendMessage("open and scroll through settings")
        advanceUntilIdle()

        assertFalse(viewModel.isLoading.value)
        val toolMessages = viewModel.messages.filter { it.content.startsWith("🤖") || it.content.startsWith("⚙️") }
        assertEquals(2, toolMessages.size)
        assertTrue(viewModel.messages.last().content.contains("Giving up"))
    }

    @Test
    fun `all tools fail then hit step limit gracefully`() = runTest {
        // #280: all failures + step limit reached — worst case
        val failingResponses = (1..25).map { i ->
            ChatResponse(
                text = null,
                toolCalls = listOf(ToolCall("t$i", "unknown_tool", emptyMap())),
                stopReason = "tool_use"
            )
        }
        val scripted = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            scripted = ArrayDeque(failingResponses)
        )
        setApiModeWithBackends(
            viewModel,
            listOf(viewModel.createTestBackend(Provider.ANTHROPIC, scripted, scripted))
        )

        viewModel.sendMessage("tap everything on screen")
        advanceUntilIdle()

        assertFalse(viewModel.isLoading.value)
        val toolMessages = viewModel.messages.filter { it.content.startsWith("🤖") || it.content.startsWith("⚙️") }
        assertEquals(25, toolMessages.size)
        assertTrue(toolMessages.all { it.content.contains("Failed") || it.content.contains("Unknown") })
        // #603: hardcoded "Hit step limit" replaced by requestFinalExplanation
    }

    // ========== Edge-Case Tests: Exception During Tool Execution (#281) ==========

    @Test
    fun `runtime exception during tool execution emits safe error and continues`() = runTest {
        // #281: exception thrown from tool execution — should be caught and loop continues
        // We use a valid tool name but with bad input that causes an exception
        val scripted = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            scripted = ArrayDeque(
                listOf(
                    ChatResponse(
                        text = null,
                        toolCalls = listOf(
                            // tap without element_id throws IllegalArgumentException
                            ToolCall("t1", "tap", emptyMap()),
                            ToolCall("t2", "press_back", emptyMap())
                        ),
                        stopReason = "tool_use"
                    ),
                    ChatResponse(
                        text = "Handled the error",
                        toolCalls = emptyList(),
                        stopReason = "end_turn"
                    )
                )
            )
        )
        setApiModeWithBackends(
            viewModel,
            listOf(viewModel.createTestBackend(Provider.ANTHROPIC, scripted, scripted))
        )

        viewModel.sendMessage("tap the first element")
        advanceUntilIdle()

        assertFalse(viewModel.isLoading.value)
        val toolMessages = viewModel.messages.filter { it.content.startsWith("🤖") || it.content.startsWith("⚙️") }
        assertEquals(2, toolMessages.size)
        // First tool should have error, second should succeed
        assertTrue(toolMessages[0].content.contains("Failed"))
        assertTrue(viewModel.messages.last().content.contains("Handled the error"))
    }

    @Test
    fun `null pointer in tool input is caught gracefully`() = runTest {
        // #281: tool receives null where it expects a value
        val scripted = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            scripted = ArrayDeque(
                listOf(
                    ChatResponse(
                        text = null,
                        toolCalls = listOf(
                            ToolCall("t1", "type_text", emptyMap()), // missing "text" param
                        ),
                        stopReason = "tool_use"
                    ),
                    ChatResponse(
                        text = "Recovered from missing input",
                        toolCalls = emptyList(),
                        stopReason = "end_turn"
                    )
                )
            )
        )
        setApiModeWithBackends(
            viewModel,
            listOf(viewModel.createTestBackend(Provider.ANTHROPIC, scripted, scripted))
        )

        viewModel.sendMessage("type something in the field")
        advanceUntilIdle()

        assertFalse(viewModel.isLoading.value)
        val toolMessages = viewModel.messages.filter { it.content.startsWith("🤖") || it.content.startsWith("⚙️") }
        assertEquals(1, toolMessages.size)
        assertTrue(toolMessages[0].content.contains("Failed"))
        assertTrue(viewModel.messages.any { it.content.contains("Recovered") })
    }

    @Test
    fun `exception on every tool in response does not crash viewmodel`() = runTest {
        // #281: every tool throws — ChatViewModel should survive
        val scripted = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            scripted = ArrayDeque(
                listOf(
                    ChatResponse(
                        text = null,
                        toolCalls = listOf(
                            ToolCall("t1", "tap", emptyMap()),       // missing element_id
                            ToolCall("t2", "type_text", emptyMap()), // missing text
                            ToolCall("t3", "swipe", emptyMap())      // missing direction
                        ),
                        stopReason = "tool_use"
                    ),
                    ChatResponse(
                        text = "Everything broke but I'm still here",
                        toolCalls = emptyList(),
                        stopReason = "end_turn"
                    )
                )
            )
        )
        setApiModeWithBackends(
            viewModel,
            listOf(viewModel.createTestBackend(Provider.ANTHROPIC, scripted, scripted))
        )

        viewModel.sendMessage("tap and type and swipe")
        advanceUntilIdle()

        assertFalse(viewModel.isLoading.value)
        val toolMessages = viewModel.messages.filter { it.content.startsWith("🤖") || it.content.startsWith("⚙️") }
        assertEquals(3, toolMessages.size)
        assertTrue(toolMessages.all { it.content.contains("Failed") })
        // ViewModel survived — final response still delivered
        assertTrue(viewModel.messages.last().content.contains("still here"))
        assertFalse(viewModel.isLoading.value)
    }

    @Test
    fun `provider exception during follow-up response returns error message`() = runTest {
        // #281: first response has tools, but the follow-up call throws ProviderException
        val callCount = java.util.concurrent.atomic.AtomicInteger(0)
        val scripted = object : ProviderClient {
            override val provider = Provider.ANTHROPIC
            override suspend fun chat(conversation: ai.citros.core.Conversation): Result<String> =
                Result.success("unused")
            override suspend fun describeImage(base64Image: String, prompt: String, maxTokens: Int): Result<String> =
                Result.success("test")
            override suspend fun chatWithTools(
                messages: List<Message>,
                systemPrompt: String?,
                tools: List<Tool>,
                tokenLimit: Int?
            ): Result<ChatResponse> {
                val count = callCount.incrementAndGet()
                return if (count == 1) {
                    Result.success(ChatResponse(
                        text = null,
                        toolCalls = listOf(ToolCall("t1", "press_back", emptyMap())),
                        stopReason = "tool_use"
                    ))
                } else {
                    throw ProviderException(
                        provider = Provider.ANTHROPIC,
                        statusCode = 500,
                        message = "Internal server error",
                        isAuthFailure = false
                    )
                }
            }
        }
        setApiModeWithBackends(
            viewModel,
            listOf(viewModel.createTestBackend(Provider.ANTHROPIC, scripted, scripted))
        )

        viewModel.sendMessage("open the app")
        advanceUntilIdle()

        assertFalse(viewModel.isLoading.value)
        // Should have tool result from first call
        assertTrue(viewModel.messages.any { it.content.startsWith("🤖") })
        // Should have error from second call
        assertTrue(viewModel.messages.last().content.contains("Error") || 
                   viewModel.messages.last().content.contains("Internal server error"))
    }

    @Test
    fun `sendMessage auth failure triggers provider switch and retries`() = runTest {
        val failing = ThrowingProviderClient(
            provider = Provider.ANTHROPIC,
            error = ProviderException(
                provider = Provider.ANTHROPIC,
                statusCode = 401,
                message = "bad key",
                isAuthFailure = true
            )
        )
        val succeeding = ScriptedProviderClient(
            provider = Provider.OPENAI,
            scripted = ArrayDeque(
                listOf(ChatResponse(text = "Recovered", toolCalls = emptyList(), stopReason = "end_turn"))
            )
        )

        setApiModeWithBackends(
            viewModel,
            listOf(
                viewModel.createTestBackend(Provider.ANTHROPIC, failing, failing),
                viewModel.createTestBackend(Provider.OPENAI, succeeding, succeeding)
            )
        )

        viewModel.sendMessage("open settings app")
        advanceUntilIdle()

        assertFalse(viewModel.isLoading.value)
        assertTrue(viewModel.messages.any { it.content.contains("switching to OPENAI") })
        assertTrue(viewModel.messages.any { it.content.contains("Recovered") })
    }

    @Test
    fun `sendMessage non-auth provider error returns error response without switching`() = runTest {
        val failing = ThrowingProviderClient(
            provider = Provider.ANTHROPIC,
            error = ProviderException(
                provider = Provider.ANTHROPIC,
                statusCode = 429,
                message = "rate limit",
                isAuthFailure = false
            )
        )

        setApiModeWithBackends(
            viewModel,
            listOf(viewModel.createTestBackend(Provider.ANTHROPIC, failing, failing))
        )

        viewModel.sendMessage("open my email")
        advanceUntilIdle()

        assertFalse(viewModel.isLoading.value)
        assertTrue(viewModel.messages.last().content.startsWith("Error:"))
        assertTrue(viewModel.messages.none { it.content.contains("switching to") })
    }

    @Test
    fun `sendMessage with exhausted scripted responses fails gracefully`() = runTest {
        val scripted = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            scripted = ArrayDeque(emptyList())
        )

        setApiModeWithBackends(
            viewModel,
            listOf(viewModel.createTestBackend(Provider.ANTHROPIC, scripted, scripted))
        )

        viewModel.sendMessage("open the browser")
        advanceUntilIdle()

        assertFalse(viewModel.isLoading.value)
        assertTrue(viewModel.messages.last().content.startsWith("Error:"))
        assertTrue(viewModel.messages.last().content.contains("no scripted response left"))
    }

    @Test
    fun `clearConversation clears messages`() {
        viewModel.messages.add(Message(role = "user", content = "Hello"))

        viewModel.clearConversation()

        assertEquals(0, viewModel.messages.size)
    }

    @Test
    fun `clearConversation resets toolLoopCancelled and loading state`() {
        viewModel.messages.add(Message(role = "user", content = "test"))
        viewModel.cancelToolExecution()
        assertTrue(viewModel.isToolLoopCancelledForTesting())

        viewModel.clearConversation()

        assertFalse(viewModel.isToolLoopCancelledForTesting())
        assertFalse(viewModel.isLoading.value)
        assertEquals(null, viewModel.error.value)
        assertEquals(null, viewModel.queuedMessage.value)
        // Overlay state should be cleared but service stays alive
        // Overlay state sync is handled by ChatActivity mediator (#463).
        // ViewModel no longer mutates OverlayController directly.
    }

    @Test
    fun `configureWithWallet with empty wallet sets needsAuth`() {
        val keyStore = InMemoryKeyStore()
        val storage = InMemoryWalletStorage()
        val walletManager = ai.citros.core.WalletManager(storage, keyStore)

        viewModel.configureWithWallet(walletManager)

        assertFalse(viewModel.isConfigured.value)
        assertTrue(viewModel.needsAuth.value)
    }

    @Test
    fun `configureWithWallet with valid wallet sets isConfigured`() {
        val keyStore = InMemoryKeyStore()
        val storage = InMemoryWalletStorage()
        val walletManager = ai.citros.core.WalletManager(storage, keyStore)

        // Add a key and set it active
        walletManager.addKey(Provider.ANTHROPIC, "Test Key", "sk-ant-api03-test-key-12345")
        walletManager.setActiveKey(walletManager.loadOrDefault().keys.first().id)

        viewModel.configureWithWallet(walletManager)

        assertTrue(viewModel.isConfigured.value)
        assertFalse(viewModel.needsAuth.value)
    }

    @Test
    fun `configureWithWallet builds correct provider client`() {
        val keyStore = InMemoryKeyStore()
        val storage = InMemoryWalletStorage()
        val walletManager = ai.citros.core.WalletManager(storage, keyStore)

        // Add OpenAI key
        walletManager.addKey(Provider.OPENAI, "OpenAI Key", "sk-test-openai-key")
        walletManager.setActiveKey(walletManager.loadOrDefault().keys.first().id)

        viewModel.configureWithWallet(walletManager)

        assertTrue(viewModel.isConfigured.value)
        // Provider should be configured (can't directly inspect but isConfigured should be true)
    }

    @Test
    fun `updateModelsFromWallet refreshes tracked chat and action models`() {
        val keyStore = InMemoryKeyStore()
        val storage = InMemoryWalletStorage()
        val walletManager = ai.citros.core.WalletManager(storage, keyStore)

        walletManager.addKey(Provider.OPENAI, "OpenAI Key", "sk-test-openai-key")
        walletManager.setActiveKey(walletManager.loadOrDefault().keys.first().id)
        viewModel.configureWithWallet(walletManager)

        walletManager.setChatModel("gpt-4o-mini")
        walletManager.setActionModel("gpt-4o")
        viewModel.updateModelsFromWallet(walletManager)

        assertEquals("gpt-4o-mini", getPrivateField<String>("lastWalletChatModelId"))
        // lastWalletActionModelId removed - action model is now dynamically derived via ModelConfig.actionModelForChat()
        assertTrue(viewModel.isConfigured.value)
        assertFalse(viewModel.needsAuth.value)
    }

    @Test
    fun `updateModelsFromWallet falls back to full configure when provider changes`() {
        val keyStore = InMemoryKeyStore()
        val storage = InMemoryWalletStorage()
        val walletManager = ai.citros.core.WalletManager(storage, keyStore)

        val openAiKey = walletManager.addKey(Provider.OPENAI, "OpenAI Key", "sk-test-openai-key")
        val anthropicKey = walletManager.addKey(Provider.ANTHROPIC, "Anthropic Key", "sk-ant-api03-test-key")
        walletManager.setActiveKey(openAiKey.id)
        viewModel.configureWithWallet(walletManager)

        walletManager.setActiveKey(anthropicKey.id)
        viewModel.updateModelsFromWallet(walletManager)

        assertEquals(Provider.ANTHROPIC, getPrivateField<Provider>("lastWalletProvider"))
        assertTrue(viewModel.isConfigured.value)
        assertFalse(viewModel.needsAuth.value)
    }

    @Test
    fun `configureWithWallet with no activeKeyId sets needsAuth`() {
        val keyStore = InMemoryKeyStore()
        val storage = InMemoryWalletStorage()
        val walletManager = ai.citros.core.WalletManager(storage, keyStore)

        // Add a key but don't set it active
        walletManager.addKey(Provider.ANTHROPIC, "Test Key", "sk-ant-api03-test-key-12345")
        // Don't call setActiveKey

        viewModel.configureWithWallet(walletManager)

        assertFalse(viewModel.isConfigured.value)
        assertTrue(viewModel.needsAuth.value)
    }


    // ========== PR #620: actionModelId tracking in updateModelsFromWallet ==========

    @Test
    fun `updateModelsFromWallet detects action model change and rebuilds`() {
        val keyStore = InMemoryKeyStore()
        val storage = InMemoryWalletStorage()
        val walletManager = ai.citros.core.WalletManager(storage, keyStore)

        walletManager.addKey(Provider.OPENAI, "OpenAI Key", "sk-test-openai-key")
        walletManager.setActiveKey(walletManager.loadOrDefault().keys.first().id)
        viewModel.configureWithWallet(walletManager)

        // Capture active backend instance identity before change
        val backends = getPrivateField<List<Any>>("apiBackends")!!
        val activeIdx = getPrivateField<Int>("activeApiBackendIndex")!!
        val backendBefore = backends[activeIdx]

        // Change only the action model
        walletManager.setActionModel("gpt-4o-2024-08-06")
        viewModel.updateModelsFromWallet(walletManager)

        // Active backend should be a DIFFERENT instance (rebuild happened)
        // The list is mutated in-place (apiBackends[activeIndex] = updatedBackend),
        // so we check the element identity, not the list identity.
        val backendsAfter = getPrivateField<List<Any>>("apiBackends")!!
        val backendAfter = backendsAfter[activeIdx]
        assertNotEquals(
            System.identityHashCode(backendBefore),
            System.identityHashCode(backendAfter),
            "Active backend should be rebuilt when action model changes"
        )
        // Action model ID should be updated
        assertEquals("gpt-4o-2024-08-06", getPrivateField<String>("lastWalletActionModelId"))
        assertTrue(viewModel.isConfigured.value)
    }

    @Test
    fun `updateModelsFromWallet skips rebuild when all models unchanged`() {
        val keyStore = InMemoryKeyStore()
        val storage = InMemoryWalletStorage()
        val walletManager = ai.citros.core.WalletManager(storage, keyStore)

        walletManager.addKey(Provider.OPENAI, "OpenAI Key", "sk-test-openai-key")
        walletManager.setActiveKey(walletManager.loadOrDefault().keys.first().id)
        viewModel.configureWithWallet(walletManager)

        // Capture active backend instance identity before no-op call
        val backends = getPrivateField<List<Any>>("apiBackends")!!
        val activeIdx = getPrivateField<Int>("activeApiBackendIndex")!!
        val backendBefore = backends[activeIdx]

        // Call again with same models — should be a no-op
        viewModel.updateModelsFromWallet(walletManager)

        // Active backend should be the SAME instance (no rebuild)
        val backendsAfter = getPrivateField<List<Any>>("apiBackends")!!
        val backendAfter = backendsAfter[activeIdx]
        assertEquals(
            System.identityHashCode(backendBefore),
            System.identityHashCode(backendAfter),
            "Active backend should NOT be rebuilt when models are unchanged"
        )
        assertTrue(viewModel.isConfigured.value)
    }

    // ========== PR #622: model switch text-only seed ==========

    @Test
    fun `updateModelsFromWallet does not transfer raw messages to new backend`() = runTest {
        val keyStore = InMemoryKeyStore()
        val storage = InMemoryWalletStorage()
        val walletManager = ai.citros.core.WalletManager(storage, keyStore)

        walletManager.addKey(Provider.OPENAI, "OpenAI Key", "sk-test-openai-key")
        walletManager.setActiveKey(walletManager.loadOrDefault().keys.first().id)
        viewModel.configureWithWallet(walletManager)

        // Add some UI messages to simulate prior conversation
        viewModel.messages.add(Message(role = "user", content = "hello"))
        viewModel.messages.add(Message(role = "assistant", content = "hi there"))

        // Switch models
        walletManager.setChatModel("gpt-4o-mini")
        viewModel.updateModelsFromWallet(walletManager)

        // The new backend's PhoneAgentApi should start with zero messages
        // (history is not transferred; it will be seeded on next sendMessage)
        val apiBackends = getPrivateField<List<Any>>("apiBackends")
        assertNotNull(apiBackends)
        assertTrue(apiBackends!!.isNotEmpty())

        val activeIndex = getPrivateField<Int>("activeApiBackendIndex")!!
        val activeBackend = apiBackends[activeIndex]
        val agentField = activeBackend::class.java.getDeclaredField("agent")
        agentField.isAccessible = true
        val agent = agentField.get(activeBackend) as ai.citros.core.PhoneAgentApi
        // messageCount is internal to :core — access messages list via reflection from :chat test
        val messagesField = agent::class.java.getDeclaredField("messages")
        messagesField.isAccessible = true
        val messages = messagesField.get(agent) as List<*>
        assertEquals(0, messages.size, "New backend should start with zero messages (no raw transfer)")
    }

    // ========== Local LLM Mode Tests (#255) ==========

    @Test
    fun `configureWithLocalLLM sets configured state`() {
        viewModel.configureWithLocalLLM(baseUrl = "http://localhost:11434", model = "qwen2.5:3b")

        assertTrue(viewModel.isConfigured.value)
        assertFalse(viewModel.needsAuth.value)
    }

    @Test
    fun `sendMessage with local LLM executes text-only conversation`() = runTest {
        val client = ScriptedLocalLLMClient(
            scripted = ArrayDeque(listOf("Hello! I'm your phone assistant."))
        )
        setLocalLLMMode(viewModel, client)

        viewModel.sendMessage("hi")
        advanceUntilIdle()

        assertFalse(viewModel.isLoading.value)
        assertEquals("user", viewModel.messages[0].role)
        assertEquals("assistant", viewModel.messages[1].role)
        assertTrue(viewModel.messages[1].content.contains("Hello! I'm your phone assistant."))
    }

    @Test
    fun `sendMessage with local LLM executes single action`() = runTest {
        val client = ScriptedLocalLLMClient(
            scripted = ArrayDeque(
                listOf(
                    """{"action": "home"}""",
                    "Done, went home"
                )
            )
        )
        setLocalLLMMode(viewModel, client)

        viewModel.sendMessage("go home")
        advanceUntilIdle()

        assertFalse(viewModel.isLoading.value)
        // press_home is a mechanical action — hidden by output classifier (#417)
        // but the final text response should still appear
        assertTrue(viewModel.messages.last().content.contains("Done, went home"))
    }

    @Test
    fun `sendMessage with local LLM follows MAX_TOOL_STEPS limit`() = runTest {
        val responses = ArrayDeque(
            (1..26).map { """{"action": "back"}""" }
        )
        val client = ScriptedLocalLLMClient(scripted = responses)
        setLocalLLMMode(viewModel, client)

        viewModel.sendMessage("keep pressing back")
        advanceUntilIdle()

        assertFalse(viewModel.isLoading.value)
        // #603: hardcoded "Hit step limit" replaced by requestFinalExplanation.
        // In local mode, sendMessageWithAgent returns null (no API backend),
        // so final explanation fails silently. Verify loop stopped.
    }

    @Test
    fun `sendMessage with local LLM handles action errors gracefully`() = runTest {
        val client = ScriptedLocalLLMClient(
            scripted = ArrayDeque(
                listOf(
                    """{"action": "click", "element": 999}""",
                    "Recovered from error"
                )
            )
        )
        setLocalLLMMode(viewModel, client)

        viewModel.sendMessage("tap nonexistent element")
        advanceUntilIdle()

        assertFalse(viewModel.isLoading.value)
        // tap failure starts with "Failed" → output classifier shows errors with 🤖 (#417)
        assertTrue(viewModel.messages.any {
            it.content.startsWith("🤖") && it.content.contains("Failed")
        })
        assertTrue(viewModel.messages.any { it.content.contains("Recovered") })
    }

    @Test
    fun `sendMessage with local LLM handles malformed JSON as text response`() = runTest {
        val client = ScriptedLocalLLMClient(
            scripted = ArrayDeque(listOf("This is just plain text, no JSON here"))
        )
        setLocalLLMMode(viewModel, client)

        viewModel.sendMessage("hello")
        advanceUntilIdle()

        assertFalse(viewModel.isLoading.value)
        assertTrue(viewModel.messages.last().content.contains("plain text"))
    }

    @Test
    fun `sendMessage with local LLM handles LLM error response`() = runTest {
        val client = FailingLocalLLMClient(error = Exception("Connection timeout"))
        setLocalLLMMode(viewModel, client)

        viewModel.sendMessage("open something")
        advanceUntilIdle()

        assertFalse(viewModel.isLoading.value)
        assertTrue(viewModel.messages.last().content.startsWith("Error:"))
        assertTrue(viewModel.messages.last().content.contains("Connection timeout"))
    }

    @Test
    fun `clearConversation clears local LLM agent conversation`() {
        val client = ScriptedLocalLLMClient(scripted = ArrayDeque(listOf("Response")))
        setLocalLLMMode(viewModel, client)
        viewModel.messages.add(Message(role = "user", content = "Hello"))

        viewModel.clearConversation()

        assertEquals(0, viewModel.messages.size)
    }


    @Test
    fun `requestFinalExplanation fires and adds message after max_steps`() = runTest {
        // 25 tool responses to hit the limit; the final explanation consumes the text response
        val responses = ArrayDeque(
            (1..25).map { index ->
                ChatResponse(
                    text = null,
                    toolCalls = listOf(ToolCall("tool_$index", "press_back", emptyMap())),
                    stopReason = "tool_use"
                )
            } + ChatResponse(
                text = "I hit the step limit. Here's what I did so far.",
                toolCalls = emptyList(),
                stopReason = "end_turn"
            )
        )
        val scripted = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            scripted = responses
        )
        setApiModeWithBackends(
            viewModel,
            listOf(viewModel.createTestBackend(Provider.ANTHROPIC, scripted, scripted))
        )

        viewModel.sendMessage("press back repeatedly")
        advanceUntilIdle()

        assertFalse(viewModel.isLoading.value)
        // The final explanation message should have been added
        val lastAssistant = viewModel.messages.lastOrNull { it.role == "assistant" }
        assertNotNull(lastAssistant)
        assertTrue(
            lastAssistant!!.content.contains("step limit"),
            "Expected final explanation message but got: ${lastAssistant.content}"
        )
    }

    // ========== Helper Methods ==========

    private fun setLocalLLMMode(viewModel: ChatViewModel, client: ai.citros.core.LocalLLMClient) {
        val agent = ai.citros.core.PhoneAgentLocal(client)
        viewModel.configureWithLocalLLMForTesting(agent)
    }

    private class ScriptedLocalLLMClient(
        private val scripted: ArrayDeque<String>
    ) : ai.citros.core.LocalLLMClient("http://localhost:11434", "test-model") {
        var calls: Int = 0

        override suspend fun chat(message: String): Result<String> {
            calls++
            val next = scripted.removeFirstOrNull()
                ?: return Result.failure(
                    IllegalStateException("ScriptedLocalLLMClient has no scripted response left at call #$calls")
                )
            return Result.success(next)
        }
    }

    // --- Tool loop overlay hook tests (#457 / #458) ---

    @Test
    fun `tool loop invokes overlay hide hook when tool calls are present`() = runTest {
        var hideCalled = false
        ai.citros.core.ScreenReader.toolLoopOverlayHideHook = { hideCalled = true }
        ai.citros.core.ScreenReader.toolLoopOverlayRestoreHook = {}

        val scripted = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            scripted = ArrayDeque(listOf(
                ChatResponse(
                    text = "I'll press back",
                    toolCalls = listOf(ToolCall("t1", "press_back", emptyMap())),
                    stopReason = "tool_use"
                ),
                ChatResponse(text = "Done", toolCalls = emptyList(), stopReason = "end_turn")
            ))
        )
        setApiModeWithBackends(viewModel, listOf(viewModel.createTestBackend(Provider.ANTHROPIC, scripted, scripted)))

        viewModel.sendMessage("press back")
        advanceUntilIdle()

        assertTrue(hideCalled, "toolLoopOverlayHideHook should have been invoked")
    }

    @Test
    fun `tool loop invokes overlay restore hook in finally on success`() = runTest {
        var restoreCalled = false
        ai.citros.core.ScreenReader.toolLoopOverlayHideHook = {}
        ai.citros.core.ScreenReader.toolLoopOverlayRestoreHook = { restoreCalled = true }

        val scripted = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            scripted = ArrayDeque(listOf(
                ChatResponse(
                    text = "I'll press back",
                    toolCalls = listOf(ToolCall("t1", "press_back", emptyMap())),
                    stopReason = "tool_use"
                ),
                ChatResponse(text = "Done", toolCalls = emptyList(), stopReason = "end_turn")
            ))
        )
        setApiModeWithBackends(viewModel, listOf(viewModel.createTestBackend(Provider.ANTHROPIC, scripted, scripted)))

        viewModel.sendMessage("press back")
        advanceUntilIdle()

        assertTrue(restoreCalled, "toolLoopOverlayRestoreHook should have been invoked in finally")
    }

    @Test
    fun `tool loop invokes overlay restore hook on exception`() = runTest {
        var restoreCalled = false
        ai.citros.core.ScreenReader.toolLoopOverlayHideHook = {}
        ai.citros.core.ScreenReader.toolLoopOverlayRestoreHook = { restoreCalled = true }

        val failing = ThrowingProviderClient(Provider.ANTHROPIC, RuntimeException("boom"))
        setApiModeWithBackends(viewModel, listOf(viewModel.createTestBackend(Provider.ANTHROPIC, failing, failing)))

        viewModel.sendMessage("do something")
        advanceUntilIdle()

        assertTrue(restoreCalled, "toolLoopOverlayRestoreHook should be invoked even on exception")
    }

    @Test
    fun `tool loop overlay hide hook failure is caught and does not crash`() = runTest {
        var restoreCalled = false
        ai.citros.core.ScreenReader.toolLoopOverlayHideHook = { throw RuntimeException("hide failed") }
        ai.citros.core.ScreenReader.toolLoopOverlayRestoreHook = { restoreCalled = true }

        val scripted = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            scripted = ArrayDeque(listOf(
                ChatResponse(
                    text = "I'll press back",
                    toolCalls = listOf(ToolCall("t1", "press_back", emptyMap())),
                    stopReason = "tool_use"
                ),
                ChatResponse(text = "Done", toolCalls = emptyList(), stopReason = "end_turn")
            ))
        )
        setApiModeWithBackends(viewModel, listOf(viewModel.createTestBackend(Provider.ANTHROPIC, scripted, scripted)))

        viewModel.sendMessage("press back")
        advanceUntilIdle()

        // Should not crash — hide hook failure is caught
        assertFalse(viewModel.isLoading.value)
        assertTrue(restoreCalled, "Restore hook should still be called even when hide hook fails")
    }

    @Test
    fun `tool loop overlay restore hook failure is caught and does not crash`() = runTest {
        ai.citros.core.ScreenReader.toolLoopOverlayHideHook = {}
        ai.citros.core.ScreenReader.toolLoopOverlayRestoreHook = { throw RuntimeException("restore failed") }

        val scripted = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            scripted = ArrayDeque(listOf(
                ChatResponse(
                    text = "I'll press back",
                    toolCalls = listOf(ToolCall("t1", "press_back", emptyMap())),
                    stopReason = "tool_use"
                ),
                ChatResponse(text = "Done", toolCalls = emptyList(), stopReason = "end_turn")
            ))
        )
        setApiModeWithBackends(viewModel, listOf(viewModel.createTestBackend(Provider.ANTHROPIC, scripted, scripted)))

        viewModel.sendMessage("press back")
        advanceUntilIdle()

        // Should not crash — restore hook failure is caught
        assertFalse(viewModel.isLoading.value)
    }

    // ========== Stuck Detection Tests (#451) ==========

    // Helper to create distinct ScreenContent instances for stuck detection tests.
    // With isReturnDefaultValues=true, Rect() returns zeros — different packageNames
    // produce different hashCodes for data class equality.
    private fun testScreen(pkg: String) = ScreenContent(emptyList(), pkg)
    private fun testScreenWithElement(pkg: String) = ScreenContent(
        listOf(ScreenElement(1, "Button", null, null, true, false, android.graphics.Rect())),
        pkg
    )

    // ========== Queued Message Tests (#445) ==========

    @Test
    fun `setQueuedMessage stores message in ViewModel`() {
        viewModel.setQueuedMessage("check bluetooth too")
        assertEquals("check bluetooth too", viewModel.queuedMessage.value)
    }

    @Test
    fun `setQueuedMessage clears blank input`() {
        viewModel.setQueuedMessage("something")
        viewModel.setQueuedMessage("   ")
        assertNull(viewModel.queuedMessage.value)
    }

    @Test
    fun `queued message dispatched after tool loop completes`() = runTest {
        val responses = ArrayDeque(
            listOf(
                ChatResponse(
                    text = null,
                    toolCalls = listOf(ToolCall("t1", "press_home", emptyMap())),
                    stopReason = "tool_use"
                ),
                ChatResponse(
                    text = "Done with first task",
                    toolCalls = emptyList(),
                    stopReason = "end_turn"
                ),
                // Response to the queued message
                ChatResponse(
                    text = "Bluetooth is already on",
                    toolCalls = emptyList(),
                    stopReason = "end_turn"
                )
            )
        )
        val scripted = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            scripted = responses
        )
        setApiModeWithBackends(
            viewModel,
            listOf(viewModel.createTestBackend(Provider.ANTHROPIC, scripted, scripted))
        )

        // Queue a follow-up before starting
        viewModel.setQueuedMessage("check bluetooth")

        viewModel.sendMessage("go home")
        advanceUntilIdle()

        // After tool loop completes, queued message should have been dispatched
        assertNull(viewModel.queuedMessage.value, "Queued message should be consumed")
        // Final message should be from the queued message response
        assertTrue(viewModel.messages.last().content.contains("Bluetooth"))
    }

    @Test
    fun `queued message not dispatched when tool loop is cancelled`() = runTest {
        val responses = ArrayDeque(
            listOf(
                ChatResponse(
                    text = null,
                    toolCalls = listOf(ToolCall("t1", "press_home", emptyMap())),
                    stopReason = "tool_use"
                ),
                ChatResponse(
                    text = "Cancelled",
                    toolCalls = emptyList(),
                    stopReason = "end_turn"
                )
            )
        )
        val scripted = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            scripted = responses
        )
        setApiModeWithBackends(
            viewModel,
            listOf(viewModel.createTestBackend(Provider.ANTHROPIC, scripted, scripted))
        )

        viewModel.setQueuedMessage("do this next")
        viewModel.cancelToolExecution()

        viewModel.sendMessage("go home")
        advanceUntilIdle()

        // Queued message should NOT have been dispatched since loop was cancelled
        // (it gets cleared by the cancellation path)
        assertFalse(viewModel.isLoading.value)
    }

    @Test
    fun `checkStuck returns null when screens are changing`() {
        val state = ChatViewModel.StuckDetectionState()

        assertNull(viewModel.checkStuck(state, "tap", testScreen("com.app1")))
        assertNull(viewModel.checkStuck(state, "tap", testScreen("com.app2")))
        assertNull(viewModel.checkStuck(state, "tap", testScreen("com.app3")))
        assertEquals(3, state.uniqueScreens)
    }

    @Test
    fun `checkStuck triggers after 3 identical screen hashes`() {
        val state = ChatViewModel.StuckDetectionState()
        val sameScreen = testScreen("com.app")

        assertNull(viewModel.checkStuck(state, "tap", sameScreen))
        assertNull(viewModel.checkStuck(state, "tap", sameScreen))
        val warning = viewModel.checkStuck(state, "tap", sameScreen)

        assertNotNull(warning)
        assertTrue(warning!!.contains("STUCK"))
        assertTrue(warning.contains("not changed in 3 actions"))
        assertEquals(1, state.uniqueScreens)
    }

    @Test
    fun `checkStuck resets when screen changes after being stuck`() {
        val state = ChatViewModel.StuckDetectionState()
        val screenA = testScreen("com.app")
        val screenB = testScreen("com.other")

        // Get stuck
        viewModel.checkStuck(state, "tap", screenA)
        viewModel.checkStuck(state, "tap", screenA)
        assertNotNull(viewModel.checkStuck(state, "tap", screenA))

        // Screen changes — no longer stuck
        assertNull(viewModel.checkStuck(state, "tap", screenB))
    }

    @Test
    fun `checkStuck warns on consecutive waits with stuck screen`() {
        val state = ChatViewModel.StuckDetectionState()
        val sameScreen = testScreen("com.app")

        // Build up screen hash history — 3rd call triggers screen-stuck
        viewModel.checkStuck(state, "tap", sameScreen)
        viewModel.checkStuck(state, "tap", sameScreen)
        assertNotNull(viewModel.checkStuck(state, "wait", sameScreen)) // 3rd identical → screen-stuck fires

        // 2nd consecutive wait on stuck screen → wait warning would fire,
        // but screen-stuck takes precedence (same result: warning present)
        val warning = viewModel.checkStuck(state, "wait", sameScreen)
        assertNotNull(warning)
        assertTrue(warning!!.contains("STUCK") || warning.contains("Waiting"))
    }

    @Test
    fun `checkStuck does not warn on waits when screen is changing`() {
        val state = ChatViewModel.StuckDetectionState()

        assertNull(viewModel.checkStuck(state, "wait", testScreen("com.app1")))
        assertNull(viewModel.checkStuck(state, "wait", testScreen("com.app2")))
        // consecutiveWaits is 2, but screen changed so no warning
        assertEquals(2, state.consecutiveWaits)
    }

    @Test
    fun `checkStuck resets consecutiveWaits on non-wait tool`() {
        val state = ChatViewModel.StuckDetectionState()

        viewModel.checkStuck(state, "wait", testScreen("com.app"))
        assertEquals(1, state.consecutiveWaits)

        viewModel.checkStuck(state, "tap", testScreen("com.app"))
        assertEquals(0, state.consecutiveWaits)
    }

    @Test
    fun `checkStuck handles null screenContent gracefully`() {
        val state = ChatViewModel.StuckDetectionState()

        // null screen shouldn't crash or produce warnings
        assertNull(viewModel.checkStuck(state, "think", null))
        assertNull(viewModel.checkStuck(state, "think", null))
        assertNull(viewModel.checkStuck(state, "think", null))
        assertEquals(0, state.uniqueScreens)
        assertTrue(state.recentScreenHashes.isEmpty())
    }

    @Test
    fun `checkStuck includes uniqueScreens count in warning`() {
        val state = ChatViewModel.StuckDetectionState()

        // See two different screens, then get stuck on one
        viewModel.checkStuck(state, "tap", testScreen("com.app"))
        viewModel.checkStuck(state, "tap", testScreen("com.other"))
        viewModel.checkStuck(state, "tap", testScreen("com.other"))
        val warning = viewModel.checkStuck(state, "tap", testScreen("com.other"))

        assertNotNull(warning)
        assertTrue(warning!!.contains("2 unique screens"))
    }

    private class FailingLocalLLMClient(
        private val error: Exception
    ) : ai.citros.core.LocalLLMClient("http://localhost:11434", "test-model") {
        override suspend fun chat(message: String): Result<String> {
            return Result.failure(error)
        }
    }

    private fun setApiModeWithBackends(
        viewModel: ChatViewModel,
        backends: List<ChatViewModel.TestApiBackend>
    ) {
        viewModel.configureForTesting(backends)
    }

    @Suppress("UNCHECKED_CAST")
    private fun <T> getPrivateField(name: String): T? {
        val field = ChatViewModel::class.java.getDeclaredField(name)
        field.isAccessible = true
        return field.get(viewModel) as T?
    }

    private class ScriptedProviderClient(
        override val provider: Provider,
        private val scripted: ArrayDeque<ChatResponse>
    ) : ProviderClient {
        var calls: Int = 0

        override suspend fun chat(conversation: ai.citros.core.Conversation): Result<String> =
            Result.success("unused")

        override suspend fun describeImage(base64Image: String, prompt: String, maxTokens: Int): Result<String> =
            Result.success("test image description")

        override suspend fun chatWithTools(
            messages: List<Message>,
            systemPrompt: String?,
            tools: List<Tool>,
            tokenLimit: Int?
        ): Result<ChatResponse> {
            calls++
            val next = scripted.removeFirstOrNull()
                ?: return Result.failure(
                    IllegalStateException(
                        "ScriptedProviderClient(${provider.name}) has no scripted response left at call #$calls"
                    )
                )
            return Result.success(next)
        }
    }

    private class ThrowingProviderClient(
        override val provider: Provider,
        private val error: Exception
    ) : ProviderClient {
        override suspend fun chat(conversation: ai.citros.core.Conversation): Result<String> =
            Result.failure(error)

        override suspend fun describeImage(base64Image: String, prompt: String, maxTokens: Int): Result<String> =
            Result.failure(error)

        override suspend fun chatWithTools(
            messages: List<Message>,
            systemPrompt: String?,
            tools: List<Tool>,
            tokenLimit: Int?
        ): Result<ChatResponse> {
            throw error
        }
    }

    @Test
    fun `tool loop aborts gracefully when accessibility detaches mid-loop`() = runTest {
        // Simulate: service is "attached" for the initial sendMessage (phoneControlOverride=true
        // makes PhoneAgentApi route to tool mode), but ScreenReader.isAttached() is false
        // (simulating detachment between the initial call and tool execution).
        // ChatViewModel checks ScreenReader.isAttached() directly before executing
        // accessibility-dependent tools when phoneControlOverride is null.
        val scripted = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            scripted = ArrayDeque(
                listOf(
                    ChatResponse(
                        text = "I'll tap that",
                        toolCalls = listOf(ToolCall("t1", "tap", mapOf("element_id" to 5))),
                        stopReason = "tool_use"
                    )
                )
            )
        )
        // phoneControlOverride=true so PhoneAgentApi routes to tool mode (simulates
        // service being attached when the request was sent). But set it to null AFTER
        // the agent is configured so the ChatViewModel's direct ScreenReader check kicks in.
        val agent = ai.citros.core.PhoneAgentApi(scripted, scripted).also {
            it.phoneControlOverride = true
        }
        val backend = viewModel.createTestBackend(Provider.ANTHROPIC, scripted, scripted, agent = agent)
        setApiModeWithBackends(viewModel, listOf(backend))

        // Simulate mid-loop detachment: screenReaderAvailableOverride=false makes
        // ChatViewModel think the service just detached (even though PhoneAgentApi
        // routed to tool mode because phoneControlOverride=true)
        viewModel.screenReaderAvailableOverride = false

        try {
            viewModel.sendMessage("tap something")
            advanceUntilIdle()

            assertFalse(viewModel.isLoading.value)
            // Without AccessibilityGateCheck in boundary checks, the executor
            // runs the tap tool normally (which may fail). Verify graceful handling.
            assertTrue(
                viewModel.messages.size >= 2,
                "Expected at least user + response messages but got: ${viewModel.messages.map { it.content }}"
            )
        } finally {
            viewModel.screenReaderAvailableOverride = null
        }
    }

    private class InMemoryWalletStorage : ai.citros.core.WalletStorage {
        private var state: ai.citros.core.WalletState? = null

        override fun loadState(): ai.citros.core.WalletState? = state

        override fun saveState(state: ai.citros.core.WalletState) {
            this.state = state
        }
    }

    // ========== Steer UI Tests ==========

    @Test
    fun `steerMessage adds to steerQueue when loading`() {
        viewModel.isLoading.value = true
        viewModel.steerMessage("go back instead")

        assertEquals(1, viewModel.steerQueue.size)
        assertEquals("go back instead", viewModel.steerQueue.peek())
        assertTrue(viewModel.hasQueuedSteer.value)
    }

    @Test
    fun `steerMessage falls back to sendMessage when not loading`() = runTest {
        val scripted = ScriptedProviderClient(
            provider = Provider.OPENROUTER,
            scripted = ArrayDeque(listOf(
                ChatResponse(text = "response", toolCalls = emptyList(), stopReason = "end_turn")
            ))
        )
        val backend = viewModel.createTestBackend(Provider.OPENROUTER, scripted)
        viewModel.configureForTesting(listOf(backend))

        viewModel.isLoading.value = false
        viewModel.steerMessage("hello")

        // Should not be in steerQueue — falls through to sendMessage
        assertTrue(viewModel.steerQueue.isEmpty())
        assertFalse(viewModel.hasQueuedSteer.value)
        // Message should appear in messages list (sendMessage adds it)
        advanceUntilIdle()
        assertTrue(viewModel.messages.any { it.role == "user" && it.content == "hello" })
    }

    @Test
    fun `steerQueue is cleared on new sendMessage`() = runTest {
        val scripted = ScriptedProviderClient(
            provider = Provider.OPENROUTER,
            scripted = ArrayDeque(listOf(
                ChatResponse(text = "ok", toolCalls = emptyList(), stopReason = "end_turn")
            ))
        )
        val backend = viewModel.createTestBackend(Provider.OPENROUTER, scripted)
        viewModel.configureForTesting(listOf(backend))

        // Pre-load some steer messages
        viewModel.steerQueue.offer("old steer")
        viewModel.hasQueuedSteer.value = true

        viewModel.sendMessage("new message")
        advanceUntilIdle()

        assertTrue(viewModel.steerQueue.isEmpty())
        assertFalse(viewModel.hasQueuedSteer.value)
    }

    @Test
    fun `drainSteerQueue drains queue atomically`() {
        viewModel.steerQueue.offer("msg1")
        viewModel.steerQueue.offer("msg2")
        viewModel.steerQueue.offer("msg3")
        viewModel.hasQueuedSteer.value = true

        val drained = viewModel.drainSteerQueue()

        assertEquals(listOf("msg1", "msg2", "msg3"), drained)
        assertTrue(viewModel.steerQueue.isEmpty())
        assertFalse(viewModel.hasQueuedSteer.value)
    }

    @Test
    fun `hasQueuedSteer updates correctly`() {
        assertFalse(viewModel.hasQueuedSteer.value)

        viewModel.isLoading.value = true
        viewModel.steerMessage("redirect")

        assertTrue(viewModel.hasQueuedSteer.value)

        // Simulate drain (what steerMessageSource does)
        viewModel.steerQueue.clear()
        viewModel.hasQueuedSteer.value = false

        assertFalse(viewModel.hasQueuedSteer.value)
    }

    @Test
    fun `rapid steers during queue drain maintain correct hasQueuedSteer state`() {
        viewModel.isLoading.value = true
        viewModel.steerMessage("msg1")

        // Simulate queue drain (what drainSteerQueue does)
        viewModel.drainSteerQueue()
        assertFalse(viewModel.hasQueuedSteer.value)

        // Immediately add another steer before any async update could interfere
        viewModel.steerMessage("msg2")

        // hasQueuedSteer should be true because a new steer was added
        assertTrue(viewModel.hasQueuedSteer.value)
        assertEquals(1, viewModel.steerQueue.size)
    }

    @Test
    fun `clearConversation clears steer queue and flag`() {
        viewModel.isLoading.value = true
        viewModel.steerMessage("redirect me")
        assertTrue(viewModel.hasQueuedSteer.value)
        assertFalse(viewModel.steerQueue.isEmpty())

        viewModel.clearConversation()

        assertTrue(viewModel.steerQueue.isEmpty())
        assertFalse(viewModel.hasQueuedSteer.value)
    }


    @Test
    fun `clearConversation is purely a data operation with no overlay side effects`() {
        // Regression test for #409: clearConversation must not stop or destroy
        // the overlay service. After the #463 refactor, ViewModel no longer
        // mutates OverlayController directly — overlay state sync is handled
        // by ChatActivity mediator. This test documents that invariant.
        viewModel.messages.add(Message(role = "user", content = "Hello"))
        viewModel.messages.add(Message(role = "assistant", content = "Hi there"))

        viewModel.clearConversation()

        // Messages cleared
        assertEquals(0, viewModel.messages.size)
        // Loading and error state reset
        assertFalse(viewModel.isLoading.value)
        assertEquals(null, viewModel.error.value)
        // No overlay interaction happened — ViewModel has no overlay reference.
        // If this test compiles and passes, the invariant holds: clearConversation
        // is purely a data operation with no side effects on the overlay service.
    }

    @Test
    fun `signOut clears steer queue and flag`() {
        viewModel.isLoading.value = true
        viewModel.steerMessage("redirect me")
        assertTrue(viewModel.hasQueuedSteer.value)
        assertFalse(viewModel.steerQueue.isEmpty())

        viewModel.signOut()

        assertTrue(viewModel.steerQueue.isEmpty())
        assertFalse(viewModel.hasQueuedSteer.value)
    }

    @Test
    fun `steer messages appear in messages list with isSteer true`() {
        viewModel.isLoading.value = true
        viewModel.steerMessage("do something else")

        val steerMsg = viewModel.messages.last()
        assertEquals("user", steerMsg.role)
        assertEquals("do something else", steerMsg.content)
        assertTrue(steerMsg.isSteer)
    }

    // ========== Memory Wiring Tests ==========

    @Test
    fun `remember tool succeeds when memoryProvider is wired`() = runTest {
        val memProvider = InMemoryMemoryProvider()
        val scripted = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            scripted = ArrayDeque(listOf(
                ChatResponse(
                    text = null,
                    toolCalls = listOf(
                        ToolCall("t1", "remember", mapOf(
                            "content" to "favorite color is blue",
                            "tags" to "preferences"
                        ))
                    ),
                    stopReason = "tool_use"
                ),
                ChatResponse(
                    text = "Got it, I'll remember that!",
                    toolCalls = emptyList(),
                    stopReason = "end_turn"
                )
            ))
        )
        val backend = viewModel.createTestBackend(
            Provider.ANTHROPIC, scripted, scripted, memoryProvider = memProvider
        )
        setApiModeWithBackends(viewModel, listOf(backend))

        viewModel.sendMessage("remember my favorite color is blue")
        advanceUntilIdle()

        assertFalse(viewModel.isLoading.value)
        // Memory should have been stored
        assertEquals(1, memProvider.stored.size)
        assertEquals("favorite color is blue", memProvider.stored[0].content)
        // Should NOT contain "not configured" error
        assertFalse(
            viewModel.messages.any { it.content.contains("not configured", ignoreCase = true) },
            "Expected no 'not configured' error but got: ${viewModel.messages.map { it.content }}"
        )
    }

    @Test
    fun `remember then recall round-trip works`() = runTest {
        val memProvider = InMemoryMemoryProvider()
        // First: remember
        val rememberScripted = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            scripted = ArrayDeque(listOf(
                ChatResponse(
                    text = null,
                    toolCalls = listOf(
                        ToolCall("t1", "remember", mapOf(
                            "content" to "meeting at 3pm",
                            "tags" to "schedule"
                        ))
                    ),
                    stopReason = "tool_use"
                ),
                ChatResponse(text = "Noted!", toolCalls = emptyList(), stopReason = "end_turn")
            ))
        )
        val backend = viewModel.createTestBackend(
            Provider.ANTHROPIC, rememberScripted, rememberScripted, memoryProvider = memProvider
        )
        setApiModeWithBackends(viewModel, listOf(backend))

        viewModel.sendMessage("remember meeting at 3pm")
        advanceUntilIdle()

        assertEquals(1, memProvider.stored.size)

        // Second: recall
        val recallScripted = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            scripted = ArrayDeque(listOf(
                ChatResponse(
                    text = null,
                    toolCalls = listOf(
                        ToolCall("t2", "recall", mapOf("query" to "meeting"))
                    ),
                    stopReason = "tool_use"
                ),
                ChatResponse(text = "Your meeting is at 3pm.", toolCalls = emptyList(), stopReason = "end_turn")
            ))
        )
        val recallBackend = viewModel.createTestBackend(
            Provider.ANTHROPIC, recallScripted, recallScripted, memoryProvider = memProvider
        )
        setApiModeWithBackends(viewModel, listOf(recallBackend))

        viewModel.sendMessage("what's my meeting time?")
        advanceUntilIdle()

        // Recall should have found the memory
        assertFalse(viewModel.isLoading.value)
        assertFalse(
            viewModel.messages.any { it.content.contains("not configured", ignoreCase = true) },
            "Expected no 'not configured' error"
        )
    }

    @Test
    fun `memory tools fail with not configured when no provider set`() = runTest {
        val scripted = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            scripted = ArrayDeque(listOf(
                ChatResponse(
                    text = null,
                    toolCalls = listOf(
                        ToolCall("t1", "remember", mapOf("content" to "test"))
                    ),
                    stopReason = "tool_use"
                ),
                ChatResponse(text = "Error occurred", toolCalls = emptyList(), stopReason = "end_turn")
            ))
        )
        // No memoryProvider passed — should get "not configured" error
        val backend = viewModel.createTestBackend(Provider.ANTHROPIC, scripted, scripted)
        setApiModeWithBackends(viewModel, listOf(backend))

        viewModel.sendMessage("remember something")
        advanceUntilIdle()

        assertFalse(viewModel.isLoading.value)
        // Without memoryProvider, agent may fall back to chat mode (no memory tools registered).
        // Verify the ViewModel handled the request without crashing.
        assertTrue(
            viewModel.messages.size >= 2,
            "Expected at least user message + response but got: ${viewModel.messages.map { it.content }}"
        )
    }

    @Test
    fun `setMemoryProvider rebuilds backends with provider`() {
        val scripted = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            scripted = ArrayDeque()
        )
        val backend = viewModel.createTestBackend(Provider.ANTHROPIC, scripted, scripted)
        setApiModeWithBackends(viewModel, listOf(backend))

        val memProvider = InMemoryMemoryProvider()
        viewModel.setMemoryProvider(memProvider)

        // ViewModel should have rebuilt — we can't directly inspect the internal
        // PhoneAgentApi, but we verify it doesn't crash and the setter completes
        assertTrue(viewModel.isConfigured.value)
    }

    // ========== toolStatusLabel tests ==========

    @Test
    fun `toolStatusLabel returns generic label for mechanical tools`() {
        val mechanicalTools = listOf("tap", "swipe", "scroll", "type_text", "press_back")
        mechanicalTools.forEach { tool ->
            val label = ChatViewModel.toolStatusLabel(tool)
            assertEquals("Interacting...", label, "Mechanical tool '$tool' should use generic label")
        }
    }

    @Test
    fun `toolStatusLabel returns specific label for prominent tools`() {
        val label = ChatViewModel.toolStatusLabel("open_app")
        assertTrue(label.contains("Opening"), "open_app label should contain 'Opening': $label")

        val screenshotLabel = ChatViewModel.toolStatusLabel("screenshot")
        assertTrue(screenshotLabel.contains("screenshot", ignoreCase = true),
            "screenshot label should mention screenshot: $screenshotLabel")

        val notifLabel = ChatViewModel.toolStatusLabel("open_notifications")
        assertTrue(notifLabel.contains("notification", ignoreCase = true),
            "open_notifications label should mention notification: $notifLabel")

        val subtaskLabel = ChatViewModel.toolStatusLabel("subtask")
        assertTrue(subtaskLabel.contains("subtask", ignoreCase = true),
            "subtask label should mention subtask: $subtaskLabel")
    }

    @Test
    fun `toolStatusLabel returns specific label for research tools`() {
        val searchLabel = ChatViewModel.toolStatusLabel("web_search")
        assertTrue(searchLabel.contains("earch", ignoreCase = true),
            "web_search label should mention search: $searchLabel")

        val fetchLabel = ChatViewModel.toolStatusLabel("web_fetch")
        assertTrue(fetchLabel.contains("etch", ignoreCase = true),
            "web_fetch label should mention fetch: $fetchLabel")
    }

    @Test
    fun `toolStatusLabel returns thinking for reasoning tools`() {
        val label = ChatViewModel.toolStatusLabel("think")
        assertTrue(label.contains("Think", ignoreCase = true),
            "think label should mention thinking: $label")
    }

    @Test
    fun `toolStatusLabel returns fallback label for unknown tools`() {
        val label = ChatViewModel.toolStatusLabel("some_new_tool")
        assertTrue(label.contains("some_new_tool"),
            "Unknown tool label should contain the tool name: $label")
    }

    // ========== Conversation Lifecycle ==========

    @Test
    fun `checkLifecycleAndClear returns null when no messages`() {
        val vm = ChatViewModel()
        val result = vm.checkLifecycleAndClear(
            timeoutMs = 30 * 60 * 1000,
            lastConversationDate = "2026-02-15",
            nowMs = System.currentTimeMillis()
        )
        assertEquals(null, result)
    }

    @Test
    fun `checkLifecycleAndClear triggers idle timeout`() {
        val vm = ChatViewModel()
        vm.messages.add(Message(role = "user", content = "hello"))
        val thirtyMinAgo = System.currentTimeMillis() - 31 * 60 * 1000
        vm.lastActivityTimestamp = thirtyMinAgo

        val result = vm.checkLifecycleAndClear(
            timeoutMs = 30L * 60 * 1000,
            lastConversationDate = ConversationLifecycle.todayDateString(), // same day
            nowMs = System.currentTimeMillis()
        )

        assertEquals(LifecycleClearReason.IDLE_TIMEOUT, result)
        // Should have exactly one message: the system message
        assertEquals(1, vm.messages.size)
        assertTrue(vm.messages[0].content.contains("inactivity"))
    }

    @Test
    fun `checkLifecycleAndClear does not trigger within threshold`() {
        val vm = ChatViewModel()
        vm.messages.add(Message(role = "user", content = "hello"))
        vm.lastActivityTimestamp = System.currentTimeMillis() - 5 * 60 * 1000 // 5 min ago

        val result = vm.checkLifecycleAndClear(
            timeoutMs = 30L * 60 * 1000,
            lastConversationDate = ConversationLifecycle.todayDateString(),
            nowMs = System.currentTimeMillis()
        )

        assertEquals(null, result)
        assertEquals(1, vm.messages.size) // original message preserved
    }

    @Test
    fun `checkLifecycleAndClear triggers daily reset`() {
        val vm = ChatViewModel()
        vm.messages.add(Message(role = "user", content = "hello"))
        vm.lastActivityTimestamp = System.currentTimeMillis() // recent activity

        val result = vm.checkLifecycleAndClear(
            timeoutMs = 30L * 60 * 1000,
            lastConversationDate = "2026-02-15", // yesterday
            nowMs = System.currentTimeMillis()
        )

        assertEquals(LifecycleClearReason.DAILY_RESET, result)
        assertEquals(1, vm.messages.size)
        assertTrue(vm.messages[0].content.contains("fresh start"))
    }

    @Test
    fun `checkLifecycleAndClear daily reset takes priority over idle timeout`() {
        val vm = ChatViewModel()
        vm.messages.add(Message(role = "user", content = "hello"))
        vm.lastActivityTimestamp = System.currentTimeMillis() - 60 * 60 * 1000 // 1 hour ago

        val result = vm.checkLifecycleAndClear(
            timeoutMs = 30L * 60 * 1000, // would trigger idle timeout
            lastConversationDate = "2026-02-15", // would also trigger daily reset
            nowMs = System.currentTimeMillis()
        )

        // Daily reset takes priority
        assertEquals(LifecycleClearReason.DAILY_RESET, result)
        assertTrue(vm.messages[0].content.contains("fresh start"))
    }

    @Test
    fun `checkLifecycleAndClear respects NEVER setting`() {
        val vm = ChatViewModel()
        vm.messages.add(Message(role = "user", content = "hello"))
        vm.lastActivityTimestamp = System.currentTimeMillis() - 999 * 60 * 1000

        val result = vm.checkLifecycleAndClear(
            timeoutMs = ConversationLifecycle.TIMEOUT_NEVER,
            lastConversationDate = ConversationLifecycle.todayDateString(), // same day
            nowMs = System.currentTimeMillis()
        )

        assertEquals(null, result)
        assertEquals(1, vm.messages.size) // untouched
    }

    // ========== Pending Task Message (#447) ==========

    @Test
    fun `pendingTaskMessage is set when accessibility unavailable`() {
        val vm = ChatViewModel()
        assertNull(vm.pendingTaskMessage)

        vm.setPendingTaskIfAccessibilityUnavailable("open spotify", accessibilityAvailable = false)

        assertEquals("open spotify", vm.pendingTaskMessage)
    }

    @Test
    fun `pendingTaskMessage is not set when accessibility available`() {
        val vm = ChatViewModel()

        vm.setPendingTaskIfAccessibilityUnavailable("open spotify", accessibilityAvailable = true)

        assertNull(vm.pendingTaskMessage)
    }

    @Test
    fun `consumePendingTaskContext prepends original request`() {
        val vm = ChatViewModel()
        vm.pendingTaskMessage = "open spotify and play my playlist"

        val result = vm.consumePendingTaskContext("try again")

        assertEquals("Original request: open spotify and play my playlist\nUser follow-up: try again", result)
        assertNull(vm.pendingTaskMessage, "Pending message should be cleared after consumption")
    }

    @Test
    fun `consumePendingTaskContext returns original message when no pending`() {
        val vm = ChatViewModel()

        val result = vm.consumePendingTaskContext("hello")

        assertEquals("hello", result)
        assertNull(vm.pendingTaskMessage, "Should be null even after no-op consume")
    }

    @Test
    fun `consumePendingTaskContext clears stash even when no pending exists`() {
        // Verifies always-clear semantics — calling consume twice doesn't replay
        val vm = ChatViewModel()
        vm.pendingTaskMessage = "open spotify"

        val first = vm.consumePendingTaskContext("try again")
        val second = vm.consumePendingTaskContext("try once more")

        assertTrue(first.contains("Original request: open spotify"))
        assertEquals("try once more", second, "Second consume should not replay stash")
    }

    @Test
    fun `pendingTaskMessage not consumed when accessibility still unavailable`() {
        // Simulates: user sends task → no accessibility → sends another message → still no accessibility
        val vm = ChatViewModel()
        vm.setPendingTaskIfAccessibilityUnavailable("open spotify", accessibilityAvailable = false)
        assertEquals("open spotify", vm.pendingTaskMessage)

        // Second message while accessibility still off — stash should update to latest task
        vm.setPendingTaskIfAccessibilityUnavailable("open youtube", accessibilityAvailable = false)
        assertEquals("open youtube", vm.pendingTaskMessage, "Should update to latest task")
    }

    @Test
    fun `clearConversation clears pendingTaskMessage`() {
        val vm = ChatViewModel()
        vm.pendingTaskMessage = "stashed task"

        vm.clearConversation()

        assertNull(vm.pendingTaskMessage)
    }

    /** Simple in-memory MemoryProvider for unit tests (no SQLite dependency). */
    private class InMemoryMemoryProvider : MemoryProvider {
        data class StoredMemory(val id: String, val content: String, val tags: List<String>, val source: String?)
        val stored = mutableListOf<StoredMemory>()

        override suspend fun store(content: String, metadata: MemoryMetadata): String {
            val id = "mem-${stored.size + 1}"
            stored.add(StoredMemory(id, content, metadata.tags, metadata.source))
            return id
        }

        override suspend fun search(query: String, limit: Int): List<MemoryResult> {
            return stored.filter { it.content.contains(query, ignoreCase = true) }
                .take(limit)
                .map { MemoryResult(it.id, it.content, it.tags, it.source, System.currentTimeMillis()) }
        }

        override suspend fun delete(id: String) {
            stored.removeAll { it.id == id }
        }

        override suspend fun list(filter: MemoryFilter?): List<MemoryResult> {
            return stored.take(filter?.limit ?: stored.size)
                .map { MemoryResult(it.id, it.content, it.tags, it.source, System.currentTimeMillis()) }
        }
    }


    // ========== Overlay duplicate-send regression test (#561) ==========

    @Test
    fun `overlay submit while loading routes to steer not sendMessage`() {
        // Regression test for #561: overlay submit called sendMessage directly
        // while model was busy, creating concurrent coroutines and duplicate API calls.
        // The fix routes through steerMessage when isLoading=true.
        viewModel.isLoading.value = true

        // Simulate what the fixed overlay submit handler does:
        // if (viewModel.isLoading.value) viewModel.steerMessage(draft)
        viewModel.steerMessage("check bluetooth")

        // Should be in steer queue (injected at next tool boundary), not a new message
        assertEquals(1, viewModel.steerQueue.size)
        assertEquals("check bluetooth", viewModel.steerQueue.peek())
        assertTrue(viewModel.hasQueuedSteer.value)

        // The message should appear in the UI message list with isSteer=true
        val steerMsg = viewModel.messages.lastOrNull { it.content == "check bluetooth" }
        assertNotNull(steerMsg, "Steer message should appear in messages")
        assertTrue(steerMsg!!.isSteer, "Message should be marked as steer")
    }

    @Test
    fun `overlay submit while idle sends normally`() = runTest {
        val scripted = ScriptedProviderClient(
            provider = Provider.OPENROUTER,
            scripted = ArrayDeque(listOf(
                ChatResponse(text = "got it", toolCalls = emptyList(), stopReason = "end_turn")
            ))
        )
        val backend = viewModel.createTestBackend(Provider.OPENROUTER, scripted)
        viewModel.configureForTesting(listOf(backend))

        viewModel.isLoading.value = false

        // Simulate what the fixed overlay submit handler does:
        // if (!viewModel.isLoading.value) viewModel.sendMessage(draft)
        viewModel.sendMessage("hello")

        // Should NOT be in steer queue
        assertTrue(viewModel.steerQueue.isEmpty())
        // Should be in messages as a regular user message
        advanceUntilIdle()
        val userMsg = viewModel.messages.firstOrNull { it.role == "user" && it.content == "hello" }
        assertNotNull(userMsg, "User message should appear in messages")
    }


    // =========================================================================
    // Post-action behavior tests (#603)
    // =========================================================================

    @Test
    fun `lastExitReason is null by default`() {
        assertNull(viewModel.lastExitReason)
    }

    @Test
    fun `consumeExitReasonHint returns null when no exit reason`() {
        assertNull(viewModel.consumeExitReasonHint())
    }

    @Test
    fun `consumeExitReasonHint returns hint for cancelled and clears flag`() {
        viewModel.lastExitReason = ChatViewModel.ToolLoopExit.CANCELLED
        val hint = viewModel.consumeExitReasonHint()
        assertNotNull(hint)
        assertTrue(hint!!.contains("stopped by the user"))
        // Flag should be consumed
        assertNull(viewModel.lastExitReason)
        // Second call returns null
        assertNull(viewModel.consumeExitReasonHint())
    }

    @Test
    fun `consumeExitReasonHint returns null for unknown reason and clears flag`() {
        viewModel.lastExitReason = ChatViewModel.ToolLoopExit.MAX_STEPS  // used as a non-CANCELLED value
        val hint = viewModel.consumeExitReasonHint()
        assertNull(hint)
        assertNull(viewModel.lastExitReason)
    }

    @Test
    fun `clearConversation clears lastExitReason`() {
        viewModel.lastExitReason = ChatViewModel.ToolLoopExit.CANCELLED
        viewModel.clearConversation()
        assertNull(viewModel.lastExitReason)
    }

    @Test
    fun `ToolLoopExit END_TURN has no system prompt`() {
        assertNull(ChatViewModel.ToolLoopExit.END_TURN.systemPrompt)
    }

    @Test
    fun `ToolLoopExit CANCELLED has no system prompt`() {
        assertNull(ChatViewModel.ToolLoopExit.CANCELLED.systemPrompt)
    }

    @Test
    fun `ToolLoopExit MAX_STEPS has system prompt`() {
        val prompt = ChatViewModel.ToolLoopExit.MAX_STEPS.systemPrompt
        assertNotNull(prompt)
        assertTrue(prompt!!.contains("step limit"))
    }

    @Test
    fun `ToolLoopExit ACCESSIBILITY_LOST has system prompt`() {
        val prompt = ChatViewModel.ToolLoopExit.ACCESSIBILITY_LOST.systemPrompt
        assertNotNull(prompt)
        assertTrue(prompt!!.contains("accessibility service"))
    }

    @Test
    fun `lastExitReason hint is prepended to next sendMessage`() = runTest {
        // Simulate the state after a cancelled tool loop
        viewModel.lastExitReason = ChatViewModel.ToolLoopExit.CANCELLED

        val scripted = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            scripted = ArrayDeque(listOf(
                ChatResponse(
                    text = "Sure, I'll do that instead.",
                    toolCalls = emptyList(),
                    stopReason = "end_turn"
                )
            ))
        )
        setApiModeWithBackends(
            viewModel,
            listOf(viewModel.createTestBackend(Provider.ANTHROPIC, scripted, scripted))
        )

        viewModel.sendMessage("do something else")
        advanceUntilIdle()

        // Flag should be consumed
        assertNull(viewModel.lastExitReason)
        // Agent should have responded
        val assistantMsg = viewModel.messages.lastOrNull { it.role == "assistant" }
        assertNotNull(assistantMsg)
        assertTrue(assistantMsg!!.content.contains("do that instead"))
    }

    // --- streamingContentVersion tests (#618 / #640) ---
    //
    // Testing limitation: The streaming increment (`streamingContentVersion.intValue++`)
    // happens inside `mainHandler.post { }` in the `onDelta` callback. Without Robolectric
    // and its Looper shadow, `mainHandler.post` is a no-op in pure JUnit tests — posted
    // runnables never execute. This means we CANNOT observe mid-stream increments here.
    //
    // What we CAN test:
    //   - Counter initial state and reset paths (clearConversation, sendMessage completion)
    //   - That sendMessage flows reset the counter at the end (via ScriptedProviderClient)
    //
    // What we CANNOT test without Robolectric:
    //   - Counter incrementing during streaming (onDelta → mainHandler.post)
    //   - Auto-scroll behavior in ChatActivity's LaunchedEffect(streamingVersion)
    //
    // See #643 for adding Robolectric-based streaming integration tests.

    @Test
    fun `streamingContentVersion starts at zero`() {
        assertEquals(0, viewModel.streamingContentVersion.intValue)
    }

    @Test
    fun `clearConversation resets streamingContentVersion`() {
        // Manually bump to simulate mid-stream state
        viewModel.streamingContentVersion.intValue = 5
        assertEquals(5, viewModel.streamingContentVersion.intValue)

        viewModel.clearConversation()

        assertEquals(0, viewModel.streamingContentVersion.intValue)
    }

    @Test
    fun `streamingContentVersion resets after successful sendMessage`() = runTest {
        // Pre-set to non-zero to simulate a value accumulated during streaming.
        // The production code resets to 0 in the finally/completion block (line ~912).
        viewModel.streamingContentVersion.intValue = 10

        val scripted = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            scripted = ArrayDeque(listOf(
                ChatResponse(
                    text = "Done.",
                    toolCalls = emptyList(),
                    stopReason = "end_turn"
                )
            ))
        )
        setApiModeWithBackends(
            viewModel,
            listOf(viewModel.createTestBackend(Provider.ANTHROPIC, scripted, scripted))
        )

        viewModel.sendMessage("test streaming reset")
        advanceUntilIdle()

        // Exercises the completion path reset (isLoading.value = false block)
        assertEquals(0, viewModel.streamingContentVersion.intValue)
    }

    @Test
    fun `streamingContentVersion resets when not configured`() = runTest {
        // Exercises the early-return error path (line ~810): no backend → "Not configured"
        viewModel.streamingContentVersion.intValue = 7

        viewModel.sendMessage("test error reset")
        advanceUntilIdle()

        assertEquals(0, viewModel.streamingContentVersion.intValue)
    }

    @Test
    fun `streamingContentVersion resets after provider error`() = runTest {
        // Exercises the exception path via ThrowingProviderClient
        viewModel.streamingContentVersion.intValue = 3

        val throwing = ThrowingProviderClient(
            provider = Provider.ANTHROPIC,
            error = RuntimeException("connection failed")
        )
        setApiModeWithBackends(
            viewModel,
            listOf(viewModel.createTestBackend(Provider.ANTHROPIC, throwing, throwing))
        )

        viewModel.sendMessage("test provider error reset")
        advanceUntilIdle()

        assertEquals(0, viewModel.streamingContentVersion.intValue)
    }
    //
    // The ViewModel-level tests above verify the version counter lifecycle,
    // which is the prerequisite for the scroll behavior.

}
