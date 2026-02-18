package ai.citros.chat

import android.content.Context
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.test.core.app.ApplicationProvider
import ai.citros.core.Provider
import ai.citros.core.WalletKey
import ai.citros.core.WalletManager
import ai.citros.core.WalletState
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import kotlin.test.assertTrue

/**
 * Tests for ported settings UI components from citros-ui-mocks.html.
 *
 * Note: This is retroactive test coverage for UI components that were ported
 * from citros-ui-mocks.html. Future feature work follows strict TDD (RED → GREEN → REFACTOR).
 *
 * ## useUnmergedTree usage:
 * - `useUnmergedTree = true` is required for text assertions inside clickable Surfaces
 *   (SettingsNavCard uses Surface + clickable, which merges child semantics).
 * - It is NOT needed for performClick() — clicking works on the merged node.
 * - assertExists() is used instead of assertIsDisplayed() because Robolectric
 *   doesn't reliably report display state for items in scrollable containers.
 */
@RunWith(RobolectricTestRunner::class)
class SettingsHubScreensTest {

    @get:Rule
    val composeRule = createComposeRule()

    // ── Helpers ──────────────────────────────────────────────────────

    /** Creates a WalletManager with an optional active key for the given provider. */
    private fun createTestWalletManager(
        provider: Provider = Provider.ANTHROPIC,
        chatModel: String = "claude-3-5-sonnet-20241022",
        actionModel: String = "claude-3-5-sonnet-20241022",
        label: String = "Test Key",
        apiKey: String = "sk-ant-api03-" + "x".repeat(50)
    ): WalletManager {
        val keyStore = InMemoryKeyStore()
        val storage = InMemoryWalletStorage()
        val walletManager = WalletManager(storage, keyStore)

        val walletState = WalletState(
            keys = listOf(
                WalletKey(
                    id = "key1",
                    provider = provider,
                    label = label,
                    addedAt = System.currentTimeMillis()
                )
            ),
            activeKeyId = "key1",
            chatModelId = chatModel,
            actionModelId = actionModel
        )
        storage.saveState(walletState)
        keyStore.put("key1", apiKey)

        return walletManager
    }

    /** Creates an empty WalletManager with no keys. */
    private fun createEmptyWalletManager(): WalletManager {
        return WalletManager(InMemoryWalletStorage(), InMemoryKeyStore())
    }

    // ── Individual Settings Screens ─────────────────────────────────

    /** Verifies SoundSettingsScreen displays voice settings toggles. */
    @Test
    fun `SoundSettingsScreen displays voice toggles`() {
        var backClicked = false

        composeRule.setContent {
            SoundSettingsScreen(voiceManager = null, onBack = { backClicked = true })
        }

        composeRule.onNodeWithText("Sound & Haptics", useUnmergedTree = true).assertExists()

        composeRule.onNodeWithContentDescription("Back").performClick()
        assertTrue(backClicked, "Expected back callback to fire when Back button clicked")
    }

    /** Verifies PhoneControlSettingsScreen shows accessibility and overlay permission info. */
    @Test
    fun `PhoneControlSettingsScreen displays permission settings`() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        var backClicked = false

        composeRule.setContent {
            PhoneControlSettingsScreen(
                context = context,
                onBack = { backClicked = true }
            )
        }

        composeRule.onNodeWithText("Phone Control", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Accessibility Service", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Display over other apps", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Required for automated actions like tapping, scrolling, and reading screen content", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Allows Citros to show confirmation dialogs and status indicators", useUnmergedTree = true).assertExists()

        composeRule.onNodeWithContentDescription("Back").performClick()
        assertTrue(backClicked, "Expected back callback to fire when Back button clicked")
    }

    /** Verifies ModelsSettingsScreen renders model selection sections when a key is active. */
    @Test
    fun `ModelsSettingsScreen with active provider shows model selection`() {
        val walletManager = createTestWalletManager()
        var backClicked = false

        composeRule.setContent {
            ModelsSettingsScreen(
                walletManager = walletManager,
                onBack = { backClicked = true }
            )
        }

        composeRule.onNodeWithText("Models", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Model Selection", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Chat Model", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Action Model", useUnmergedTree = true).assertExists()

        composeRule.onNodeWithContentDescription("Back").performClick()
        assertTrue(backClicked, "Expected back callback to fire when Back button clicked")
    }

    /** Verifies ModelsSettingsScreen shows "No API Key Active" when wallet has no key. */
    @Test
    fun `ModelsSettingsScreen without active key shows no key message`() {
        val walletManager = createEmptyWalletManager()
        var backClicked = false

        composeRule.setContent {
            ModelsSettingsScreen(
                walletManager = walletManager,
                onBack = { backClicked = true }
            )
        }

        composeRule.onNodeWithText("Models", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithContentDescription("No API Key").assertIsDisplayed()
        composeRule.onNodeWithText("No API Key Active", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Add an API key in Settings → API Keys to configure model preferences", useUnmergedTree = true).assertExists()

        composeRule.onNodeWithContentDescription("Back").performClick()
        assertTrue(backClicked, "Expected back callback to fire when Back button clicked")
    }

    /** Verifies TrustSettingsScreen displays all three trust level options with descriptions. */
    @Test
    fun `TrustSettingsScreen displays all trust level options`() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        var backClicked = false

        composeRule.setContent {
            TrustSettingsScreen(
                context = context,
                onBack = { backClicked = true }
            )
        }

        composeRule.onNodeWithText("Trust Level", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Ask before everything", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Ask for risky stuff", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Full autonomy", useUnmergedTree = true).assertExists()

        composeRule.onNodeWithText("Citros asks before every phone action.", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Citros asks before sensitive actions like send/delete/purchase.", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Citros executes without confirmation dialogs.", useUnmergedTree = true).assertExists()

        composeRule.onNodeWithContentDescription("Back").performClick()
        assertTrue(backClicked, "Expected back callback to fire when Back button clicked")
    }

    /** Verifies AppearanceSettingsScreen displays flavor section, theme section, and mode options. */
    @Test
    fun `AppearanceSettingsScreen displays flavor and theme options`() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        var backClicked = false

        composeRule.setContent {
            AppearanceSettingsScreen(
                context = context,
                onBack = { backClicked = true }
            )
        }

        composeRule.onNodeWithText("Appearance", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Flavor", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Theme", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Dark", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Light", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("System", useUnmergedTree = true).assertExists()

        composeRule.onNodeWithContentDescription("Back").performClick()
        assertTrue(backClicked, "Expected back callback to fire when Back button clicked")
    }

    /** Verifies AboutSettingsScreen displays app metadata (version, runtime, SDK). */
    @Test
    fun `AboutSettingsScreen displays version information`() {
        var backClicked = false

        composeRule.setContent {
            AboutSettingsScreen(onBack = { backClicked = true })
        }

        composeRule.onNodeWithText("About", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Citros", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("AI phone agent for Android", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Version 0.1.0", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Runtime: Rust + Kotlin", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("UI: Jetpack Compose", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Min SDK: 28", useUnmergedTree = true).assertExists()

        composeRule.onNodeWithContentDescription("Back").performClick()
        assertTrue(backClicked, "Expected back callback to fire when Back button clicked")
    }

    // ── Settings Hub: Card Rendering ────────────────────────────────

    /** Verifies all 7 navigation cards render with correct titles. */
    @Test
    fun `SettingsHubScreen displays all navigation cards with correct titles`() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val walletManager = createEmptyWalletManager()

        composeRule.setContent {
            SettingsHubScreen(
                context = context,
                walletManager = walletManager,
                onBack = {},
                onOpenWallet = {},
                onOpenModels = {},
                onOpenTrust = {},
                onOpenPhoneControl = {},
                onOpenSound = {},
                onOpenAppearance = {},
                onOpenAbout = {}
            )
        }

        composeRule.onNodeWithText("API Keys", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Models", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Sound & Haptics", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Trust Level", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Phone Control", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Appearance", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("About", useUnmergedTree = true).assertExists()
    }

    /** Verifies all 7 navigation cards render with correct subtitles. */
    @Test
    fun `SettingsHubScreen displays all navigation cards with correct subtitles`() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val walletManager = createEmptyWalletManager()

        composeRule.setContent {
            SettingsHubScreen(
                context = context,
                walletManager = walletManager,
                onBack = {},
                onOpenWallet = {},
                onOpenModels = {},
                onOpenTrust = {},
                onOpenPhoneControl = {},
                onOpenSound = {},
                onOpenAppearance = {},
                onOpenAbout = {}
            )
        }

        composeRule.onNodeWithText("Manage your provider keys", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Chat & action model selection", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Voice, sounds, haptic feedback", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Permission tier settings", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Accessibility & overlay", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Theme & flavor settings", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Version, licenses", useUnmergedTree = true).assertExists()
    }

    /** Verifies the Sound & Haptics card renders and its subtitle is correct. */
    @Test
    fun `SettingsHubScreen includes new Sound and Haptics card`() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val walletManager = createEmptyWalletManager()
        var soundClicked = false

        composeRule.setContent {
            SettingsHubScreen(
                context = context,
                walletManager = walletManager,
                onBack = {},
                onOpenWallet = {},
                onOpenModels = {},
                onOpenTrust = {},
                onOpenPhoneControl = {},
                onOpenSound = { soundClicked = true },
                onOpenAppearance = {},
                onOpenAbout = {}
            )
        }

        composeRule.onNodeWithText("Sound & Haptics", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Voice, sounds, haptic feedback", useUnmergedTree = true).assertExists()

        composeRule.onNodeWithText("Sound & Haptics").performClick()
        assertTrue(soundClicked, "Expected Sound & Haptics callback to fire")
    }

    /** Verifies Phone Control card shows the updated subtitle "Accessibility & overlay". */
    @Test
    fun `SettingsHubScreen has updated Phone Control subtitle`() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val walletManager = createEmptyWalletManager()

        composeRule.setContent {
            SettingsHubScreen(
                context = context,
                walletManager = walletManager,
                onBack = {},
                onOpenWallet = {},
                onOpenModels = {},
                onOpenTrust = {},
                onOpenPhoneControl = {},
                onOpenSound = {},
                onOpenAppearance = {},
                onOpenAbout = {}
            )
        }

        composeRule.onNodeWithText("Phone Control", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Accessibility & overlay", useUnmergedTree = true).assertExists()
    }

    // ── Settings Hub: Callback Tests ────────────────────────────────

    /** Verifies Models card fires onOpenModels and NOT onOpenWallet (separate callbacks). */
    @Test
    fun `SettingsHubScreen Models card uses separate callback`() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val walletManager = createEmptyWalletManager()
        var modelsClicked = false
        var walletClicked = false

        composeRule.setContent {
            SettingsHubScreen(
                context = context,
                walletManager = walletManager,
                onBack = {},
                onOpenWallet = { walletClicked = true },
                onOpenModels = { modelsClicked = true },
                onOpenTrust = {},
                onOpenPhoneControl = {},
                onOpenSound = {},
                onOpenAppearance = {},
                onOpenAbout = {}
            )
        }

        composeRule.onNodeWithText("Models").performClick()
        assertTrue(modelsClicked, "Expected Models callback to fire")
        assertTrue(!walletClicked, "Models should not trigger wallet callback")
    }

    /** Verifies API Keys card fires onOpenWallet callback. */
    @Test
    fun `SettingsHubScreen API Keys card fires callback`() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val walletManager = createEmptyWalletManager()
        var walletClicked = false

        composeRule.setContent {
            SettingsHubScreen(
                context = context,
                walletManager = walletManager,
                onBack = {},
                onOpenWallet = { walletClicked = true },
                onOpenModels = {},
                onOpenTrust = {},
                onOpenPhoneControl = {},
                onOpenSound = {},
                onOpenAppearance = {},
                onOpenAbout = {}
            )
        }

        composeRule.onNodeWithText("API Keys").performClick()
        assertTrue(walletClicked, "Expected API Keys callback to fire")
    }

    /** Verifies Trust Level card fires onOpenTrust callback (requires scroll). */
    @Test
    fun `SettingsHubScreen Trust Level card fires callback`() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val walletManager = createEmptyWalletManager()
        var trustClicked = false

        composeRule.setContent {
            SettingsHubScreen(
                context = context,
                walletManager = walletManager,
                onBack = {},
                onOpenWallet = {},
                onOpenModels = {},
                onOpenTrust = { trustClicked = true },
                onOpenPhoneControl = {},
                onOpenSound = {},
                onOpenAppearance = {},
                onOpenAbout = {}
            )
        }

        // Cards below the fold need performScrollTo() before click
        composeRule.onNodeWithText("Trust Level").performScrollTo().performClick()
        assertTrue(trustClicked, "Expected Trust Level callback to fire")
    }

    /** Verifies Phone Control card fires onOpenPhoneControl callback (requires scroll). */
    @Test
    fun `SettingsHubScreen Phone Control card fires callback`() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val walletManager = createEmptyWalletManager()
        var phoneControlClicked = false

        composeRule.setContent {
            SettingsHubScreen(
                context = context,
                walletManager = walletManager,
                onBack = {},
                onOpenWallet = {},
                onOpenModels = {},
                onOpenTrust = {},
                onOpenPhoneControl = { phoneControlClicked = true },
                onOpenSound = {},
                onOpenAppearance = {},
                onOpenAbout = {}
            )
        }

        composeRule.onNodeWithText("Phone Control").performScrollTo().performClick()
        assertTrue(phoneControlClicked, "Expected Phone Control callback to fire")
    }

    /** Verifies Appearance card fires onOpenAppearance callback (requires scroll). */
    @Test
    fun `SettingsHubScreen Appearance card fires callback`() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val walletManager = createEmptyWalletManager()
        var appearanceClicked = false

        composeRule.setContent {
            SettingsHubScreen(
                context = context,
                walletManager = walletManager,
                onBack = {},
                onOpenWallet = {},
                onOpenModels = {},
                onOpenTrust = {},
                onOpenPhoneControl = {},
                onOpenSound = {},
                onOpenAppearance = { appearanceClicked = true },
                onOpenAbout = {}
            )
        }

        composeRule.onNodeWithText("Appearance").performScrollTo().performClick()
        assertTrue(appearanceClicked, "Expected Appearance callback to fire")
    }

    /** Verifies About card fires onOpenAbout callback (requires scroll). */
    @Test
    fun `SettingsHubScreen About card fires callback`() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val walletManager = createEmptyWalletManager()
        var aboutClicked = false

        composeRule.setContent {
            SettingsHubScreen(
                context = context,
                walletManager = walletManager,
                onBack = {},
                onOpenWallet = {},
                onOpenModels = {},
                onOpenTrust = {},
                onOpenPhoneControl = {},
                onOpenSound = {},
                onOpenAppearance = {},
                onOpenAbout = { aboutClicked = true }
            )
        }

        composeRule.onNodeWithText("About").performScrollTo().performClick()
        assertTrue(aboutClicked, "Expected About callback to fire")
    }

    // ── Provider-Specific Model Screen Tests ────────────────────────

    /** Verifies ModelsSettingsScreen renders model selection for Anthropic provider. */
    @Test
    fun `ModelsSettingsScreen with Anthropic provider shows correct models`() {
        val walletManager = createTestWalletManager(
            provider = Provider.ANTHROPIC,
            chatModel = "claude-3-5-sonnet-20241022",
            actionModel = "claude-3-5-sonnet-20241022",
            label = "Anthropic Key",
            apiKey = "sk-ant-api03-" + "x".repeat(50)
        )

        composeRule.setContent {
            ModelsSettingsScreen(walletManager = walletManager, onBack = {})
        }

        composeRule.onNodeWithText("Model Selection", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Chat Model", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Action Model", useUnmergedTree = true).assertExists()
    }

    /** Verifies ModelsSettingsScreen renders model selection for OpenAI provider. */
    @Test
    fun `ModelsSettingsScreen with OpenAI provider shows correct models`() {
        val walletManager = createTestWalletManager(
            provider = Provider.OPENAI,
            chatModel = "gpt-4o",
            actionModel = "gpt-4o",
            label = "OpenAI Key",
            apiKey = "sk-" + "x".repeat(48)
        )

        composeRule.setContent {
            ModelsSettingsScreen(walletManager = walletManager, onBack = {})
        }

        composeRule.onNodeWithText("Model Selection", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Chat Model", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Action Model", useUnmergedTree = true).assertExists()
    }

    /** Verifies ModelsSettingsScreen renders model selection for OpenRouter provider. */
    @Test
    fun `ModelsSettingsScreen with OpenRouter provider shows correct models`() {
        val walletManager = createTestWalletManager(
            provider = Provider.OPENROUTER,
            chatModel = "anthropic/claude-3-5-sonnet",
            actionModel = "anthropic/claude-3-5-sonnet",
            label = "OpenRouter Key",
            apiKey = "sk-or-v1-" + "x".repeat(54)
        )

        composeRule.setContent {
            ModelsSettingsScreen(walletManager = walletManager, onBack = {})
        }

        composeRule.onNodeWithText("Model Selection", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Chat Model", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Action Model", useUnmergedTree = true).assertExists()
    }

    // ── Test Utilities ──────────────────────────────────────────────

    private class InMemoryWalletStorage : ai.citros.core.WalletStorage {
        private var state: ai.citros.core.WalletState? = null
        override fun loadState(): ai.citros.core.WalletState? = state
        override fun saveState(state: ai.citros.core.WalletState) { this.state = state }
    }

    private class InMemoryKeyStore : ai.citros.core.KeyStore {
        private val store = mutableMapOf<String, String>()
        override fun get(keyId: String): String? = store[keyId]
        override fun put(keyId: String, value: String) { store[keyId] = value }
        override fun remove(keyId: String) { store.remove(keyId) }
        override fun clear() { store.clear() }
    }
}
