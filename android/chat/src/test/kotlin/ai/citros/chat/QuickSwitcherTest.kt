package ai.citros.chat

import android.content.Context
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.semantics.SemanticsActions
import androidx.test.core.app.ApplicationProvider
import ai.citros.core.ChatResponse
import ai.citros.core.Conversation
import ai.citros.core.Message
import ai.citros.core.ModelConfig
import ai.citros.core.Provider
import ai.citros.core.ProviderClient
import ai.citros.core.ToolCall
import ai.citros.core.WalletKey
import ai.citros.core.WalletManager
import ai.citros.core.WalletState
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import kotlin.test.assertEquals

@RunWith(RobolectricTestRunner::class)
class QuickSwitcherTest {

    @get:Rule
    val composeRule = createComposeRule()

    @Test
    fun abbreviatedModelNameFormatsKnownModelIds() {
        assertEquals("Sonnet 4.5", abbreviatedModelName("claude-sonnet-4-5-latest"))
        assertEquals("Sonnet 4.5", abbreviatedModelName("anthropic/claude-sonnet-4.5"))
        assertEquals("Haiku 4.5", abbreviatedModelName("anthropic/claude-haiku-4.5"))
        assertEquals("GPT 4O", abbreviatedModelName("gpt-4o"))
    }

    @Test
    fun toolbarChipRendersProviderIconAndModelName() {
        composeRule.setContent {
            QuickSwitcherToolbarChip(
                provider = Provider.OPENROUTER,
                chatModelId = "anthropic/claude-sonnet-4.5",
                onClick = {}
            )
        }

        composeRule.onNodeWithText("🔷").assertIsDisplayed()
        composeRule.onNodeWithText("Sonnet 4.5").assertIsDisplayed()
        composeRule
            .onNodeWithContentDescription("Quick switcher. Provider OPENROUTER. Chat model Sonnet 4.5")
            .assertIsDisplayed()
    }

    @Test
    fun bottomSheetKeyAndModelTapsInvokeCallbacks() {
        val keyOne = WalletKey("k1", Provider.OPENAI, "Personal", 0L)
        val keyTwo = WalletKey("k2", Provider.ANTHROPIC, "Work", 0L)
        val state = WalletState(
            keys = listOf(keyOne, keyTwo),
            activeKeyId = "k1",
            chatModelId = "gpt-4o",
            actionModelId = "gpt-4o"
        )

        var selectedChatModel: String? = null
        var selectedActionModel: String? = null
        var openedManage = false

        composeRule.setContent {
            QuickSwitcherBottomSheet(
                walletState = state,
                onDismiss = {},
                onSelectKey = {},
                onSelectChatModel = { selectedChatModel = it },
                onSelectActionModel = { selectedActionModel = it },
                onManageKeys = { openedManage = true }
            )
        }

        composeRule
            .onNodeWithTag("quick_switcher_chat_model_gpt-4o")
            .performSemanticsAction(SemanticsActions.OnClick)
        composeRule
            .onNodeWithTag("quick_switcher_action_model_gpt-4o")
            .performSemanticsAction(SemanticsActions.OnClick)
        composeRule
            .onNodeWithTag("quick_switcher_manage_keys")
            .performSemanticsAction(SemanticsActions.OnClick)

        assertEquals("gpt-4o", selectedChatModel)
        assertEquals("gpt-4o", selectedActionModel)
        assertEquals(true, openedManage)
    }

    @Test
    fun chatScreenQuickSwitcherVisibilityAndOpenFlow() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        resetWalletStores(context)
        val testKeyStore = InMemoryKeyStore()
        val testWalletStorage = SharedPreferencesWalletStorage(context)
        val testSecureStore = InMemoryCredentialStore()
        val walletManager = WalletManager(
            storage = testWalletStorage,
            keyStore = testKeyStore
        )
        val keyId = walletManager.addKey(Provider.OPENAI, "Work", "sk-test-openai")
        walletManager.setActiveKey(keyId.id)
        walletManager.setChatModel(ModelConfig.defaultChatModel(Provider.OPENAI))
        walletManager.setActionModel(ModelConfig.defaultActionModel(Provider.OPENAI))

        val viewModel = ChatViewModel()
        val backend = viewModel.createTestBackend(
            provider = Provider.OPENAI,
            chatClient = NoopProviderClient(Provider.OPENAI)
        )
        viewModel.configureForTesting(listOf(backend))

        composeRule.setContent {
            ChatScreen(
                viewModel = viewModel,
                keyStoreOverride = testKeyStore,
                walletStorageOverride = testWalletStorage,
                secureStoreOverride = testSecureStore
            )
        }

        composeRule.onNodeWithTag("quick_switcher_chip").assertIsDisplayed()

        composeRule.onNodeWithTag("quick_switcher_chip").performClick()

        // Verify the sheet opened via a stable test tag (not copy text)
        composeRule.onNodeWithTag(TEST_TAG_QUICK_SWITCHER_SHEET).assertIsDisplayed()
    }

    @Test
    fun chatScreenHidesQuickSwitcherWhenNoActiveKey() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        resetWalletStores(context)

        val testKeyStore = InMemoryKeyStore()
        val testWalletStorage = SharedPreferencesWalletStorage(context)
        val testSecureStore = InMemoryCredentialStore()

        val viewModel = ChatViewModel()
        val backend = viewModel.createTestBackend(
            provider = Provider.OPENAI,
            chatClient = NoopProviderClient(Provider.OPENAI)
        )
        viewModel.configureForTesting(listOf(backend))

        composeRule.setContent {
            ChatScreen(
                viewModel = viewModel,
                keyStoreOverride = testKeyStore,
                walletStorageOverride = testWalletStorage,
                secureStoreOverride = testSecureStore
            )
        }

        composeRule.onAllNodesWithTag("quick_switcher_chip").assertCountEquals(0)
    }

    private fun resetWalletStores(context: Context) {
        context.getSharedPreferences("citros_wallet", Context.MODE_PRIVATE).edit().clear().commit()
        context.getSharedPreferences("citros_keystore", Context.MODE_PRIVATE).edit().clear().commit()
    }

    private class NoopProviderClient(
        override val provider: Provider
    ) : ProviderClient {
        override suspend fun chat(conversation: Conversation): Result<String> = Result.success("ok")
        override suspend fun describeImage(base64Image: String, prompt: String, maxTokens: Int): Result<String> = Result.success("ok")

        override suspend fun chatWithTools(
            messages: List<Message>,
            systemPrompt: String?,
            tools: List<ai.citros.core.Tool>,
            tokenLimit: Int?
        ): Result<ChatResponse> {
            return Result.success(ChatResponse(text = "ok", toolCalls = emptyList<ToolCall>(), stopReason = "end_turn"))
        }
    }

}
