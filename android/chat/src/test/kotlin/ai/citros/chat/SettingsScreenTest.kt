package ai.citros.chat

import android.content.Context
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipeLeft
import androidx.test.core.app.ApplicationProvider
import ai.citros.core.KeyHealth
import ai.citros.core.Provider
import ai.citros.core.WalletKey
import ai.citros.core.WalletManager
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import kotlin.test.assertEquals
import kotlin.test.assertTrue

@RunWith(RobolectricTestRunner::class)
class SettingsScreenTest {

    @get:Rule
    val composeRule = createComposeRule()

    private val context: Context
        get() = ApplicationProvider.getApplicationContext()

    private val testWalletManager: WalletManager
        get() = WalletManager(InMemoryWalletStorage(), InMemoryKeyStore())

    @Test
    fun `maskApiKey shows prefix and suffix`() {
        val masked = maskApiKey("sk-ant-api03-abcdefgh1234")
        assertTrue(masked.startsWith("sk-ant"))
        assertTrue(masked.endsWith("1234"))
    }

    @Test
    fun `defaultLabelFor maps provider labels`() {
        assertEquals("Anthropic Key", defaultLabelFor(Provider.ANTHROPIC))
        assertEquals("OpenAI Key", defaultLabelFor(Provider.OPENAI))
        assertEquals("OpenRouter Key", defaultLabelFor(Provider.OPENROUTER))
    }

    @Test
    fun `wallet key card tap invokes callback`() {
        var tapped = false
        composeRule.setContent {
            WalletKeyCard(
                key = WalletKey(
                    id = "k1",
                    provider = Provider.ANTHROPIC,
                    label = "Personal Anthropic",
                    addedAt = 0L
                ),
                maskedKey = "sk-ant...1234",
                isActive = true,
                health = KeyHealth.VALID,
                onTap = { tapped = true },
            )
        }

        composeRule.onNodeWithText("Personal Anthropic").assertExists().performClick()
        assertTrue(tapped)
    }

    // Delete test removed — WalletKeyCard no longer has onDelete parameter


    @Test
    fun `expired key shows expired text`() {
        composeRule.setContent {
            WalletKeyCard(
                key = WalletKey(
                    id = "k1",
                    provider = Provider.ANTHROPIC,
                    label = "Expired Key",
                    addedAt = 0L,
                    expiresAt = System.currentTimeMillis() - 1000L
                ),
                maskedKey = "sk-ant...1234",
                isActive = false,
                health = KeyHealth.INVALID,
                onTap = {},
            )
        }
        composeRule.onNodeWithText("⚠\uFE0F Expired", useUnmergedTree = true).assertExists()
    }

    @Test
    fun `key expiring within 7 days shows expires soon`() {
        composeRule.setContent {
            WalletKeyCard(
                key = WalletKey(
                    id = "k1",
                    provider = Provider.ANTHROPIC,
                    label = "Expiring Key",
                    addedAt = 0L,
                    expiresAt = System.currentTimeMillis() + 3 * 24 * 60 * 60 * 1000L
                ),
                maskedKey = "sk-ant...1234",
                isActive = false,
                health = KeyHealth.VALID,
                onTap = {},
            )
        }
        composeRule.onNodeWithText("⚠\uFE0F Expires soon", useUnmergedTree = true).assertExists()
    }

    @Test
    fun `key with null expiresAt shows no expiry warning`() {
        composeRule.setContent {
            WalletKeyCard(
                key = WalletKey(
                    id = "k1",
                    provider = Provider.ANTHROPIC,
                    label = "Normal Key",
                    addedAt = 0L,
                    expiresAt = null
                ),
                maskedKey = "sk-ant...1234",
                isActive = false,
                health = KeyHealth.VALID,
                onTap = {},
            )
        }
        composeRule.onNodeWithText("⚠\uFE0F Expired", useUnmergedTree = true).assertDoesNotExist()
        composeRule.onNodeWithText("⚠\uFE0F Expires soon", useUnmergedTree = true).assertDoesNotExist()
    }

    @Test
    fun `add key bottom sheet validate enabled only when key entered`() {
        composeRule.setContent {
            SettingsScreen(
                walletManager = testWalletManager,
                keyStore = InMemoryKeyStore(),
                onBack = {}
            )
        }

        composeRule.onNodeWithContentDescription("Add Key").performClick()
        composeRule.onNodeWithText("Validate Key").assertIsNotEnabled()

        // Field placeholder is provider-specific (Anthropic default in this test setup)
        composeRule.onNodeWithText("sk-ant-...").performTextInput("sk-test-12345678901234567890")

        composeRule.onNodeWithText("Validate Key").assertIsEnabled()
    }

    @Test
    @org.junit.Ignore("performTouchInput swipeLeft broken under Robolectric 4.14 (#361)")
    fun `swipe to delete opens confirmation and deletes key`() {
        val keyStore = InMemoryKeyStore()
        val storage = InMemoryWalletStorage()
        val walletManager = WalletManager(storage, keyStore)
        val created = walletManager.addKey(Provider.OPENAI, "Temp Key", "sk-test-12345678901234567890")
        walletManager.setActiveKey(created.id)

        composeRule.setContent {
            SettingsScreen(
                walletManager = walletManager,
                keyStore = keyStore,
                onBack = {}
            )
        }

        composeRule.onNodeWithText("Temp Key").performTouchInput { swipeLeft() }
        composeRule.onNodeWithText("Delete key?").assertExists()
        composeRule.onNodeWithText("Remove Temp Key from your wallet?").assertExists()
        composeRule.onNodeWithText("Cancel").performClick()
        composeRule.onNodeWithText("Temp Key").assertExists()

        composeRule.onNodeWithText("Temp Key").performTouchInput { swipeLeft() }
        composeRule.onNodeWithText("Delete").performClick()
        composeRule.onNodeWithText("Temp Key").assertDoesNotExist()
    }

    @Test
    fun `settings hub displays correct API Keys subtitle`() {
        composeRule.setContent {
            SettingsHubScreen(
                context = context,
                walletManager = testWalletManager,
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
    }

    @Test
    fun `settings hub displays correct Models subtitle`() {
        composeRule.setContent {
            SettingsHubScreen(
                context = context,
                walletManager = testWalletManager,
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
        composeRule.onNodeWithText("Chat & action model selection", useUnmergedTree = true).assertExists()
    }

    @Test
    fun `settings hub displays correct Trust Level subtitle`() {
        composeRule.setContent {
            SettingsHubScreen(
                context = context,
                walletManager = testWalletManager,
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
        composeRule.onNodeWithText("Permission tier settings", useUnmergedTree = true).assertExists()
    }

    @Test
    fun `settings hub displays correct Appearance subtitle`() {
        composeRule.setContent {
            SettingsHubScreen(
                context = context,
                walletManager = testWalletManager,
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
        composeRule.onNodeWithText("Theme & flavor settings", useUnmergedTree = true).assertExists()
    }

    @Test
    fun `settings hub displays correct About subtitle`() {
        composeRule.setContent {
            SettingsHubScreen(
                context = context,
                walletManager = testWalletManager,
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
        composeRule.onNodeWithText("Version, licenses", useUnmergedTree = true).assertExists()
    }

    private class InMemoryWalletStorage : ai.citros.core.WalletStorage {
        private var state: ai.citros.core.WalletState? = null

        override fun loadState(): ai.citros.core.WalletState? = state

        override fun saveState(state: ai.citros.core.WalletState) {
            this.state = state
        }
    }

    private class InMemoryKeyStore : ai.citros.core.KeyStore {
        private val store = mutableMapOf<String, String>()

        override fun get(keyId: String): String? = store[keyId]

        override fun put(keyId: String, value: String) {
            store[keyId] = value
        }

        override fun remove(keyId: String) {
            store.remove(keyId)
        }

        override fun clear() {
            store.clear()
        }
    }
}
