package ai.citros.chat

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import ai.citros.core.Provider
import ai.citros.core.WalletKey
import ai.citros.core.WalletState
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * Tests for [SharedPreferencesWalletStorage] backup/recovery behavior.
 *
 * Uses Robolectric for SharedPreferences access without an Android device.
 */
@RunWith(RobolectricTestRunner::class)
class SharedPreferencesWalletStorageTest {

    private lateinit var storage: SharedPreferencesWalletStorage
    private lateinit var context: Context

    private val testKey = WalletKey(
        id = "test-key-1",
        provider = Provider.ANTHROPIC,
        label = "Test Key",
        addedAt = 1000L
    )

    private val testState = WalletState(
        keys = listOf(testKey),
        activeKeyId = "test-key-1",
        chatModelId = "claude-3-opus",
        actionModelId = "claude-3-haiku"
    )

    @Before
    fun setup() {
        context = ApplicationProvider.getApplicationContext()
        storage = SharedPreferencesWalletStorage(context)
    }

    // ====== Helpers ======

    /** Corrupt the primary wallet state in SharedPreferences. */
    private fun corruptPrimaryState(value: String = "{{corrupted json}}") {
        val prefs = context.getSharedPreferences("citros_wallet", Context.MODE_PRIVATE)
        prefs.edit().putString("wallet_state", value).commit()
    }

    /** Corrupt both primary and backup wallet state. */
    private fun corruptBothStates(primary: String = "bad primary", backup: String = "bad backup") {
        val prefs = context.getSharedPreferences("citros_wallet", Context.MODE_PRIVATE)
        prefs.edit()
            .putString("wallet_state", primary)
            .putString("wallet_state_backup", backup)
            .commit()
    }

    // ====== Tests ======

    /** Verify that saveState preserves the previous state as a backup entry. */
    @Test
    fun `saveState preserves previous state as backup`() {
        // Save initial state
        storage.saveState(testState)

        // Save a second state
        val updatedState = testState.copy(
            keys = testState.keys + WalletKey(
                id = "test-key-2",
                provider = Provider.OPENAI,
                label = "Second Key",
                addedAt = 2000L
            )
        )
        storage.saveState(updatedState)

        // Verify current state is the updated one
        val loaded = storage.loadState()
        assertNotNull(loaded)
        assertEquals(2, loaded!!.keys.size)

        // Verify backup contains original state by checking prefs directly
        val prefs = context.getSharedPreferences("citros_wallet", Context.MODE_PRIVATE)
        val backupJson = prefs.getString("wallet_state_backup", null)
        assertNotNull(backupJson)
        val backupState = Json.decodeFromString<WalletState>(backupJson!!)
        assertEquals(1, backupState.keys.size)
        assertEquals("test-key-1", backupState.keys[0].id)
    }

    /** Verify that corrupted primary state falls back to backup. */
    @Test
    fun `loadState recovers from backup when primary is corrupted`() {
        // Save valid state (creates primary)
        storage.saveState(testState)

        // Save again to create a backup of the first state
        val updatedState = testState.copy(chatModelId = "claude-3-sonnet")
        storage.saveState(updatedState)

        // Corrupt primary by writing invalid JSON
        corruptPrimaryState()

        // Load should recover from backup (the original testState)
        val recovered = storage.loadState()
        assertNotNull(recovered)
        assertEquals(1, recovered!!.keys.size)
        assertEquals("test-key-1", recovered.keys[0].id)
        // Backup had the original state (before updatedState was saved)
        assertEquals("claude-3-opus", recovered.chatModelId)
    }

    /** Verify that after recovery, the backup is promoted to primary for future reads. */
    @Test
    fun `loadState promotes backup to primary after recovery`() {
        // Save valid state, then save again to create backup
        storage.saveState(testState)
        storage.saveState(testState.copy(chatModelId = "claude-3-sonnet"))

        // Corrupt primary
        corruptPrimaryState("not valid json")

        // First load: recovers from backup and promotes
        val recovered = storage.loadState()
        assertNotNull(recovered)

        // Verify primary was restored by reading prefs directly
        val prefs = context.getSharedPreferences("citros_wallet", Context.MODE_PRIVATE)
        val primaryJson = prefs.getString("wallet_state", null)
        assertNotNull(primaryJson)
        val primaryState = Json.decodeFromString<WalletState>(primaryJson!!)
        assertEquals(recovered, primaryState)

        // Second load should succeed directly from primary (no fallback needed)
        val secondLoad = storage.loadState()
        assertEquals(recovered, secondLoad)
    }

    /** Both primary and backup corrupted should return null gracefully. */
    @Test
    fun `loadState returns null when both primary and backup are corrupted`() {
        corruptBothStates()
        assertNull(storage.loadState())
    }

    /** Corrupted primary with no backup should return null. */
    @Test
    fun `loadState returns null when primary is corrupted and no backup exists`() {
        corruptPrimaryState("bad primary")
        assertNull(storage.loadState())
    }

    /** Empty storage should return null without errors. */
    @Test
    fun `loadState returns null when no state exists`() {
        assertNull(storage.loadState())
    }

    /** Verify backward-compatible serialization with the new expiresAt field. */
    @Test
    fun `saveState and loadState round-trip with expiresAt field`() {
        val keyWithExpiry = testKey.copy(expiresAt = 9999999L)
        val stateWithExpiry = testState.copy(keys = listOf(keyWithExpiry))
        storage.saveState(stateWithExpiry)

        val loaded = storage.loadState()
        assertNotNull(loaded)
        assertEquals(9999999L, loaded!!.keys[0].expiresAt)
    }

    /** Verify null expiresAt round-trips correctly (standard non-expiring keys). */
    @Test
    fun `saveState and loadState round-trip without expiresAt field`() {
        storage.saveState(testState)
        val loaded = storage.loadState()
        assertNotNull(loaded)
        assertNull(loaded!!.keys[0].expiresAt)
    }
}
