package ai.citros.chat

import android.content.Context
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertTextContains
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performImeAction
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.performTextInput
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
import kotlin.test.assertTrue

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
            .onNodeWithTag("quick_switcher_chat_section_header")
            .performSemanticsAction(SemanticsActions.OnClick)
        composeRule
            .onNodeWithTag("quick_switcher_chat_model_gpt-4o")
            .performSemanticsAction(SemanticsActions.OnClick)

        composeRule
            .onNodeWithTag("quick_switcher_action_section_header")
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
    fun bottomSheetModelSectionsAreCollapsedByDefault() {
        val state = WalletState(
            keys = listOf(WalletKey("k1", Provider.OPENAI, "Personal", 0L)),
            activeKeyId = "k1",
            chatModelId = "gpt-4o",
            actionModelId = "gpt-4o"
        )

        composeRule.setContent {
            QuickSwitcherBottomSheet(
                walletState = state,
                onDismiss = {},
                onSelectKey = {},
                onSelectChatModel = {},
                onSelectActionModel = {},
                onManageKeys = {}
            )
        }

        composeRule.onNodeWithTag("quick_switcher_chat_model_gpt-4o").assertDoesNotExist()
        composeRule.onNodeWithTag("quick_switcher_action_model_gpt-4o").assertDoesNotExist()
    }

    @Test
    fun bottomSheetChatSectionTogglesExpandedAndCollapsed() {
        val state = WalletState(
            keys = listOf(WalletKey("k1", Provider.OPENAI, "Personal", 0L)),
            activeKeyId = "k1",
            chatModelId = "gpt-4o",
            actionModelId = "gpt-4o"
        )

        composeRule.setContent {
            QuickSwitcherBottomSheet(
                walletState = state,
                onDismiss = {},
                onSelectKey = {},
                onSelectChatModel = {},
                onSelectActionModel = {},
                onManageKeys = {}
            )
        }

        composeRule.onNodeWithTag("quick_switcher_chat_model_gpt-4o").assertDoesNotExist()
        composeRule.onNodeWithTag("quick_switcher_chat_section_header").performClick()
        composeRule.onNodeWithTag("quick_switcher_chat_model_gpt-4o").assertIsDisplayed()
        composeRule.onNodeWithTag("quick_switcher_chat_section_header").performClick()
        composeRule.onNodeWithTag("quick_switcher_chat_model_gpt-4o").assertDoesNotExist()
    }

    @Test
    fun bottomSheetSectionSelectionsUseCorrectCallbacks() {
        val state = WalletState(
            keys = listOf(WalletKey("k1", Provider.OPENAI, "Personal", 0L)),
            activeKeyId = "k1",
            chatModelId = "gpt-4o",
            actionModelId = "gpt-4o"
        )

        var selectedChatModel: String? = null
        var selectedActionModel: String? = null

        composeRule.setContent {
            QuickSwitcherBottomSheet(
                walletState = state,
                onDismiss = {},
                onSelectKey = {},
                onSelectChatModel = { selectedChatModel = it },
                onSelectActionModel = { selectedActionModel = it },
                onManageKeys = {}
            )
        }

        composeRule.onNodeWithTag("quick_switcher_chat_section_header").performClick()
        composeRule.onNodeWithTag("quick_switcher_action_section_header").performClick()

        composeRule.onNodeWithTag("quick_switcher_chat_model_gpt-4o").performClick()
        composeRule.onNodeWithTag("quick_switcher_action_model_gpt-4o").performClick()

        assertEquals("gpt-4o", selectedChatModel)
        assertEquals("gpt-4o", selectedActionModel)
    }

    @Test
    fun bottomSheetBothSectionsCanBeExpandedSimultaneously() {
        val state = WalletState(
            keys = listOf(WalletKey("k1", Provider.OPENAI, "Personal", 0L)),
            activeKeyId = "k1",
            chatModelId = "gpt-4o",
            actionModelId = "gpt-4o"
        )

        composeRule.setContent {
            QuickSwitcherBottomSheet(
                walletState = state,
                onDismiss = {},
                onSelectKey = {},
                onSelectChatModel = {},
                onSelectActionModel = {},
                onManageKeys = {}
            )
        }

        composeRule.onNodeWithTag("quick_switcher_chat_section_header").performClick()
        composeRule.onNodeWithTag("quick_switcher_action_section_header").performClick()

        composeRule.onNodeWithTag("quick_switcher_chat_model_gpt-4o").assertIsDisplayed()
        composeRule.onNodeWithTag("quick_switcher_action_model_gpt-4o").assertIsDisplayed()
    }

    @Test
    fun bottomSheetSectionHeaderChevronUpdatesWithExpansionState() {
        val state = WalletState(
            keys = listOf(WalletKey("k1", Provider.OPENAI, "Personal", 0L)),
            activeKeyId = "k1",
            chatModelId = "gpt-4o",
            actionModelId = "gpt-4o"
        )

        composeRule.setContent {
            QuickSwitcherBottomSheet(
                walletState = state,
                onDismiss = {},
                onSelectKey = {},
                onSelectChatModel = {},
                onSelectActionModel = {},
                onManageKeys = {}
            )
        }

        composeRule.onNode(
            hasContentDescription("collapsed").and(
                hasAnyAncestor(hasTestTag("quick_switcher_chat_section_header"))
            )
        ).assertIsDisplayed()

        composeRule.onNodeWithTag("quick_switcher_chat_section_header").performClick()

        composeRule.onNode(
            hasContentDescription("expanded").and(
                hasAnyAncestor(hasTestTag("quick_switcher_chat_section_header"))
            )
        ).assertIsDisplayed()
    }

    @Test
    fun chatScreenQuickSwitcherVisibilityAndOpenFlow() {
        val fixture = createChatScreenFixture(withActiveKey = true, configureBackend = true)
        setChatScreenContent(fixture)

        composeRule.onNodeWithTag(TEST_TAG_QUICK_SWITCHER_CHIP).assertIsDisplayed()
        composeRule.onNodeWithTag(TEST_TAG_QUICK_SWITCHER_CHIP).performClick()
        composeRule.onNodeWithTag(TEST_TAG_QUICK_SWITCHER_SHEET).assertIsDisplayed()
    }

    @Test
    fun chatScreenConfiguredAndActiveKeyHeaderTapOpensQuickSwitcher() {
        val fixture = createChatScreenFixture(withActiveKey = true, configureBackend = true)
        setChatScreenContent(fixture)

        composeRule.onNodeWithTag(TEST_TAG_QUICK_SWITCHER_HEADER).performClick()
        composeRule.onNodeWithTag(TEST_TAG_QUICK_SWITCHER_SHEET).assertIsDisplayed()
    }

    @Test
    fun chatScreenNoProviderChipNavigatesToApiKeys() {
        val fixture = createChatScreenFixture(withActiveKey = false, configureBackend = true)
        var openedApiKeys = false

        setChatScreenContent(
            fixture = fixture,
            onOpenApiKeys = { openedApiKeys = true }
        )

        composeRule.onNodeWithText("No provider connected").assertIsDisplayed()
        composeRule.onNodeWithText("▾").assertDoesNotExist()
        composeRule.onNodeWithTag(TEST_TAG_QUICK_SWITCHER_CHIP).assertIsDisplayed()
        composeRule.onNodeWithTag(TEST_TAG_QUICK_SWITCHER_CHIP).performClick()
        composeRule.runOnIdle {
            assertTrue(openedApiKeys, "Expected no-provider chip to navigate to API keys")
        }
    }

    @Test
    fun chatScreenNoProviderHeaderNavigatesToApiKeys() {
        val fixture = createChatScreenFixture(withActiveKey = false, configureBackend = true)
        var openedApiKeys = false

        setChatScreenContent(
            fixture = fixture,
            onOpenApiKeys = { openedApiKeys = true }
        )

        composeRule.onNodeWithText("No provider connected").assertIsDisplayed()
        composeRule.onNodeWithTag(TEST_TAG_QUICK_SWITCHER_HEADER).performClick()
        composeRule.runOnIdle {
            assertTrue(openedApiKeys, "Expected no-provider header to navigate to API keys")
        }
    }

    @Test
    fun chatScreenNoProviderBlockedSendShowsModalAndRetainsDraft() {
        val fixture = createChatScreenFixture(withActiveKey = false, configureBackend = true)
        var openedApiKeys = false

        setChatScreenContent(
            fixture = fixture,
            onOpenApiKeys = { openedApiKeys = true }
        )

        editableMessageInput().performTextInput("draft message")
        composeRule.onNodeWithTag(TEST_TAG_MESSAGE_SEND_BUTTON).performClick()

        composeRule.onNodeWithTag(TEST_TAG_API_KEY_REQUIRED_MODAL).assertIsDisplayed()
        editableMessageInput().assertTextContains("draft message")

        composeRule.onNodeWithText("Connect a provider to continue", substring = true).performClick()
        composeRule.runOnIdle {
            assertTrue(openedApiKeys, "Expected setup flag CTA to route to API keys")
        }
    }

    @Test
    fun chatScreenNoProviderBlockedImeSendShowsModalAndRetainsDraft() {
        val fixture = createChatScreenFixture(withActiveKey = false, configureBackend = true)
        setChatScreenContent(fixture)

        editableMessageInput().performTextInput("ime draft")
        editableMessageInput().performImeAction()

        composeRule.onNodeWithTag(TEST_TAG_API_KEY_REQUIRED_MODAL).assertIsDisplayed()
        editableMessageInput().assertTextContains("ime draft")
    }

    @Test
    fun chatScreenNoProviderBlockedQueuedSteerShowsModalAndPreservesQueuedText() {
        val fixture = createChatScreenFixture(
            withActiveKey = false,
            configureBackend = true,
            queuedMessage = "queued follow up"
        )
        setChatScreenContent(fixture)

        composeRule.onNodeWithTag(TEST_TAG_MESSAGE_STEER_QUEUED_BUTTON).performClick()
        composeRule.onNodeWithTag(TEST_TAG_API_KEY_REQUIRED_MODAL).assertIsDisplayed()
        composeRule.onNodeWithText("queued follow up").assertIsDisplayed()
    }

    @Test
    fun chatScreenConfiguredFalseWithKeyShowsSetupRequiredAndRoutesApiKeyCtaToApiKeys() {
        val fixture = createChatScreenFixture(withActiveKey = true, configureBackend = false)
        var openedSettings = false
        var openedApiKeys = false
        setChatScreenContent(
            fixture = fixture,
            onOpenApiKeys = { openedApiKeys = true },
            onOpenSettings = { openedSettings = true }
        )
        composeRule.runOnIdle {
            fixture.viewModel.isConfigured.value = false
        }

        composeRule.onNodeWithText("▾").assertDoesNotExist()
        composeRule.onNodeWithTag(TEST_TAG_QUICK_SWITCHER_CHIP).performClick()
        composeRule.runOnIdle {
            assertTrue(openedSettings, "Expected setup-required chip to route to settings")
            openedSettings = false
        }
        composeRule.onNodeWithTag(TEST_TAG_QUICK_SWITCHER_SHEET).assertDoesNotExist()

        composeRule.onNodeWithTag(TEST_TAG_QUICK_SWITCHER_HEADER).performClick()
        composeRule.runOnIdle {
            assertTrue(openedSettings, "Expected setup-required header to route to settings")
            openedSettings = false
        }

        editableMessageInput().performTextInput("needs setup")
        composeRule.onNodeWithTag(TEST_TAG_MESSAGE_SEND_BUTTON).performClick()
        composeRule.onNodeWithTag(TEST_TAG_API_KEY_REQUIRED_MODAL).assertIsDisplayed()
        composeRule.onNodeWithText("Provider setup is incomplete.").assertIsDisplayed()
        composeRule.onNodeWithText("Connect a provider to continue", substring = true).assertIsDisplayed()
        composeRule.onNodeWithText("Connect a provider to continue", substring = true).performClick()
        composeRule.runOnIdle {
            assertTrue(openedApiKeys, "Expected setup-required flag CTA to open API keys")
        }
    }

    @Test
    fun chatScreenApiKeyModalDismissesWhenModelAccessBecomesAvailable() {
        val fixture = createChatScreenFixture(withActiveKey = true, configureBackend = true)
        val backend = fixture.viewModel.createTestBackend(
            provider = Provider.OPENAI,
            chatClient = NoopProviderClient(Provider.OPENAI)
        )
        setChatScreenContent(fixture)
        composeRule.runOnIdle {
            fixture.viewModel.isConfigured.value = false
        }

        editableMessageInput().performTextInput("unlock models")
        composeRule.onNodeWithTag(TEST_TAG_MESSAGE_SEND_BUTTON).performClick()
        composeRule.onNodeWithTag(TEST_TAG_API_KEY_REQUIRED_MODAL).assertIsDisplayed()

        composeRule.runOnIdle {
            fixture.viewModel.configureForTesting(listOf(backend))
        }
        waitUntilApiKeyModalHidden()

        composeRule.onNodeWithTag(TEST_TAG_API_KEY_REQUIRED_MODAL).assertDoesNotExist()
        composeRule.onNodeWithTag(TEST_TAG_MESSAGE_SEND_BUTTON).performClick()
        composeRule.onNodeWithTag(TEST_TAG_API_KEY_REQUIRED_MODAL).assertDoesNotExist()
    }


    private fun modalOpenSettingsButton() = composeRule.onNodeWithTag(
        TEST_TAG_API_KEY_REQUIRED_OPEN_SETTINGS,
        useUnmergedTree = true
    )

    private fun editableMessageInput() = composeRule.onNode(
        hasSetTextAction().and(hasAnyAncestor(hasTestTag(TEST_TAG_MESSAGE_INPUT_FIELD))),
        useUnmergedTree = true
    )

    private fun waitUntilApiKeyModalHidden(timeoutMillis: Long = 5_000L) {
        composeRule.waitUntil(timeoutMillis = timeoutMillis) {
            composeRule.onAllNodesWithTag(TEST_TAG_API_KEY_REQUIRED_MODAL).fetchSemanticsNodes().isEmpty()
        }
    }

    private data class ChatScreenFixture(
        val viewModel: ChatViewModel,
        val keyStore: InMemoryKeyStore,
        val walletStorage: SharedPreferencesWalletStorage,
        val secureStore: InMemoryCredentialStore
    )

    private fun createChatScreenFixture(
        withActiveKey: Boolean,
        configureBackend: Boolean,
        queuedMessage: String? = null
    ): ChatScreenFixture {
        val context = ApplicationProvider.getApplicationContext<Context>()
        resetWalletStores(context)
        val keyStore = InMemoryKeyStore()
        val walletStorage = SharedPreferencesWalletStorage(context)
        val secureStore = InMemoryCredentialStore()

        if (withActiveKey) {
            val walletManager = WalletManager(
                storage = walletStorage,
                keyStore = keyStore
            )
            val keyId = walletManager.addKey(Provider.OPENAI, "Work", "sk-test-openai")
            walletManager.setActiveKey(keyId.id)
            walletManager.setChatModel(ModelConfig.defaultChatModel(Provider.OPENAI))
            walletManager.setActionModel(ModelConfig.defaultActionModel(Provider.OPENAI))
        }

        val viewModel = ChatViewModel()
        if (configureBackend) {
            val backend = viewModel.createTestBackend(
                provider = Provider.OPENAI,
                chatClient = NoopProviderClient(Provider.OPENAI)
            )
            viewModel.configureForTesting(listOf(backend))
        }
        queuedMessage?.let(viewModel::setQueuedMessage)

        return ChatScreenFixture(
            viewModel = viewModel,
            keyStore = keyStore,
            walletStorage = walletStorage,
            secureStore = secureStore
        )
    }

    private fun setChatScreenContent(
        fixture: ChatScreenFixture,
        onOpenApiKeys: () -> Unit = {},
        onOpenSettings: () -> Unit = {}
    ) {
        composeRule.setContent {
            ChatScreen(
                viewModel = fixture.viewModel,
                keyStoreOverride = fixture.keyStore,
                walletStorageOverride = fixture.walletStorage,
                secureStoreOverride = fixture.secureStore,
                onOpenApiKeys = onOpenApiKeys,
                onOpenSettings = onOpenSettings
            )
        }
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
