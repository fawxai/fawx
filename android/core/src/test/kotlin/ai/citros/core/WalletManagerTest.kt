package ai.citros.core

import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import org.junit.Assert.*
import kotlin.concurrent.thread
import org.junit.Before
import org.junit.Test

/**
 * Tests for wallet key management, storage, and migration.
 *
 * Tests follow TDD: written before implementation to define expected behavior.
 */
class WalletManagerTest {

    private lateinit var keyStore: InMemoryKeyStore
    private lateinit var storage: InMemoryWalletStorage
    private lateinit var manager: WalletManager

    @Before
    fun setup() {
        keyStore = InMemoryKeyStore()
        storage = InMemoryWalletStorage()
        manager = WalletManager(storage, keyStore)
    }

    // ========== WalletKey Tests ==========

    @Test
    fun `WalletKey serialization round-trip`() {
        val key = WalletKey(
            id = "test-uuid-123",
            provider = Provider.ANTHROPIC,
            label = "My Anthropic Key",
            addedAt = 1707000000000L
        )

        val json = Json.encodeToString(key)
        val decoded = Json.decodeFromString<WalletKey>(json)

        assertEquals(key, decoded)
    }

    @Test
    fun `WalletKey fields are immutable`() {
        val key = WalletKey(
            id = "id1",
            provider = Provider.ANTHROPIC,
            label = "Test",
            addedAt = 12345L
        )

        // Verify data class immutability (copy creates new instance)
        val modified = key.copy(label = "Modified")
        assertNotEquals(key.label, modified.label)
        assertEquals("Test", key.label)
    }

    // ========== WalletState Tests ==========

    @Test
    fun `WalletState serialization round-trip`() {
        val state = WalletState(
            keys = listOf(
                WalletKey("id1", Provider.ANTHROPIC, "Key 1", 1000L),
                WalletKey("id2", Provider.OPENROUTER, "Key 2", 2000L)
            ),
            activeKeyId = "id1",
            chatModelId = "claude-sonnet-4-5-20250929",
            actionModelId = "claude-haiku-4-5-20251001"
        )

        val json = Json.encodeToString(state)
        val decoded = Json.decodeFromString<WalletState>(json)

        assertEquals(state, decoded)
    }

    @Test
    fun `WalletState activeConfig returns null when no active key`() {
        val state = WalletState(
            keys = emptyList(),
            activeKeyId = null,
            chatModelId = ModelConfig.CHAT_MODEL,
            actionModelId = ModelConfig.ACTION_MODEL
        )

        assertNull(state.activeConfig(keyStore))
    }

    @Test
    fun `WalletState activeConfig returns null when active key not in keyStore`() {
        val state = WalletState(
            keys = listOf(WalletKey("id1", Provider.ANTHROPIC, "Key 1", 1000L)),
            activeKeyId = "id1",
            chatModelId = ModelConfig.CHAT_MODEL,
            actionModelId = ModelConfig.ACTION_MODEL
        )
        // keyStore is empty (no raw key stored)

        assertNull(state.activeConfig(keyStore))
    }

    @Test
    fun `WalletState activeConfig resolves to correct ProviderConfig for Anthropic`() {
        val keyId = "test-id"
        val rawKey = "sk-ant-api03-test123"
        keyStore.put(keyId, rawKey)

        val state = WalletState(
            keys = listOf(WalletKey(keyId, Provider.ANTHROPIC, "Test Key", 1000L)),
            activeKeyId = keyId,
            chatModelId = "claude-sonnet-4-5-20250929",
            actionModelId = "claude-haiku-4-5-20251001"
        )

        val config = state.activeConfig(keyStore)
        assertNotNull(config)
        assertEquals(Provider.ANTHROPIC, config!!.provider)
        assertEquals("https://api.anthropic.com/v1/messages", config.baseUrl)
        assertEquals("claude-sonnet-4-5-20250929", config.chatModelId)
        assertEquals("claude-haiku-4-5-20251001", config.actionModelId)
        assertTrue(config.headers.containsKey("x-api-key"))
        assertEquals(rawKey, config.headers["x-api-key"])
    }

    @Test
    fun `WalletState activeConfig resolves to correct ProviderConfig for OpenRouter`() {
        val keyId = "test-id"
        val rawKey = "sk-or-test123"
        keyStore.put(keyId, rawKey)

        val state = WalletState(
            keys = listOf(WalletKey(keyId, Provider.OPENROUTER, "Test Key", 1000L)),
            activeKeyId = keyId,
            chatModelId = ModelConfig.OPENROUTER_CHAT_MODEL,
            actionModelId = ModelConfig.OPENROUTER_ACTION_MODEL
        )

        val config = state.activeConfig(keyStore)
        assertNotNull(config)
        assertEquals(Provider.OPENROUTER, config!!.provider)
        assertEquals("https://openrouter.ai/api/v1/chat/completions", config.baseUrl)
        assertEquals(ModelConfig.OPENROUTER_CHAT_MODEL, config.chatModelId)
        assertEquals(ModelConfig.OPENROUTER_ACTION_MODEL, config.actionModelId)
        assertTrue(config.headers.containsKey("Authorization"))
        assertEquals("Bearer $rawKey", config.headers["Authorization"])
    }

    @Test
    fun `WalletState activeConfig resolves to correct ProviderConfig for OpenAI`() {
        val keyId = "test-id"
        val rawKey = "sk-test123"
        keyStore.put(keyId, rawKey)

        val state = WalletState(
            keys = listOf(WalletKey(keyId, Provider.OPENAI, "Test Key", 1000L)),
            activeKeyId = keyId,
            chatModelId = ModelConfig.OPENAI_CHAT_MODEL,
            actionModelId = ModelConfig.OPENAI_ACTION_MODEL
        )

        val config = state.activeConfig(keyStore)
        assertNotNull(config)
        assertEquals(Provider.OPENAI, config!!.provider)
        assertEquals("https://api.openai.com/v1/chat/completions", config.baseUrl)
        assertEquals(ModelConfig.OPENAI_CHAT_MODEL, config.chatModelId)
        assertEquals(ModelConfig.OPENAI_ACTION_MODEL, config.actionModelId)
        assertTrue(config.headers.containsKey("Authorization"))
        assertEquals("Bearer $rawKey", config.headers["Authorization"])
    }

    @Test
    fun `WalletState activeConfig uses custom model IDs from state`() {
        val keyId = "test-id"
        val rawKey = "sk-ant-api03-test123"
        keyStore.put(keyId, rawKey)

        val customChatModel = "claude-opus-4-5-20251101"
        val customActionModel = "claude-sonnet-4-5-20250929"

        val state = WalletState(
            keys = listOf(WalletKey(keyId, Provider.ANTHROPIC, "Test Key", 1000L)),
            activeKeyId = keyId,
            chatModelId = customChatModel,
            actionModelId = customActionModel
        )

        val config = state.activeConfig(keyStore)
        assertNotNull(config)
        assertEquals(customChatModel, config!!.chatModelId)
        assertEquals(customActionModel, config.actionModelId)
    }

    // ========== KeyStore Tests ==========

    @Test
    fun `KeyStore put and get`() {
        keyStore.put("key1", "value1")
        assertEquals("value1", keyStore.get("key1"))
    }

    @Test
    fun `KeyStore get returns null for missing key`() {
        assertNull(keyStore.get("nonexistent"))
    }

    @Test
    fun `KeyStore remove deletes key`() {
        keyStore.put("key1", "value1")
        keyStore.remove("key1")
        assertNull(keyStore.get("key1"))
    }

    @Test
    fun `KeyStore clear removes all keys`() {
        keyStore.put("key1", "value1")
        keyStore.put("key2", "value2")
        keyStore.clear()
        assertNull(keyStore.get("key1"))
        assertNull(keyStore.get("key2"))
    }

    // ========== WalletManager Tests ==========

    @Test
    fun `loadOrDefault returns default state when storage is empty`() {
        val state = manager.loadOrDefault()
        assertTrue(state.keys.isEmpty())
        assertNull(state.activeKeyId)
        assertEquals(ModelConfig.CHAT_MODEL, state.chatModelId)
        assertEquals(ModelConfig.ACTION_MODEL, state.actionModelId)
    }

    @Test
    fun `loadOrDefault returns saved state when storage has data`() {
        val savedState = WalletState(
            keys = listOf(WalletKey("id1", Provider.ANTHROPIC, "Key 1", 1000L)),
            activeKeyId = "id1",
            chatModelId = "custom-model",
            actionModelId = "custom-action-model"
        )
        storage.saveState(savedState)
        // Store the raw key so orphan cleanup does not remove it
        keyStore.put("id1", "sk-ant-api03-test")

        val loaded = manager.loadOrDefault()
        assertEquals(savedState, loaded)
    }

    @Test
    fun `addKey auto-detects Anthropic provider from key prefix`() {
        val key = manager.addKey(Provider.ANTHROPIC, "My Key", "sk-ant-api03-test123")

        assertEquals(Provider.ANTHROPIC, key.provider)
        assertEquals("My Key", key.label)
        assertTrue(key.id.isNotBlank())
        assertTrue(key.addedAt > 0)

        // Verify raw key stored in keyStore
        assertEquals("sk-ant-api03-test123", keyStore.get(key.id))

        // Verify wallet state updated
        val state = manager.loadOrDefault()
        assertTrue(state.keys.contains(key))
    }

    @Test
    fun `addKey auto-detects OpenRouter provider from key prefix`() {
        val key = manager.addKey(Provider.OPENROUTER, "My Key", "sk-or-test123")

        assertEquals(Provider.OPENROUTER, key.provider)

        // Verify raw key stored
        assertEquals("sk-or-test123", keyStore.get(key.id))
    }

    @Test
    fun `addKey auto-detects OpenAI provider from key prefix`() {
        val key = manager.addKey(Provider.OPENAI, "My Key", "sk-test123")

        assertEquals(Provider.OPENAI, key.provider)

        // Verify raw key stored
        assertEquals("sk-test123", keyStore.get(key.id))
    }

    @Test
    fun `addKey generates unique UUIDs`() {
        val key1 = manager.addKey(Provider.ANTHROPIC, "Key 1", "sk-ant-api03-test1")
        val key2 = manager.addKey(Provider.ANTHROPIC, "Key 2", "sk-ant-api03-test2")

        assertNotEquals(key1.id, key2.id)
    }

    @Test
    fun `addKey persists state to storage`() {
        manager.addKey(Provider.ANTHROPIC, "Key 1", "sk-ant-api03-test1")

        // Create new manager with same storage to verify persistence
        val newManager = WalletManager(storage, keyStore)
        val state = newManager.loadOrDefault()

        assertEquals(1, state.keys.size)
        assertEquals("Key 1", state.keys[0].label)
    }

    @Test
    fun `removeKey removes entry from state and keyStore`() {
        val key = manager.addKey(Provider.ANTHROPIC, "Key 1", "sk-ant-api03-test1")

        manager.removeKey(key.id)

        // Verify removed from state
        val state = manager.loadOrDefault()
        assertFalse(state.keys.contains(key))

        // Verify removed from keyStore
        assertNull(keyStore.get(key.id))
    }

    @Test
    fun `removeKey clears activeKeyId if removing active key`() {
        val key = manager.addKey(Provider.ANTHROPIC, "Key 1", "sk-ant-api03-test1")
        manager.setActiveKey(key.id)

        manager.removeKey(key.id)

        val state = manager.loadOrDefault()
        assertNull(state.activeKeyId)
    }

    @Test
    fun `removeKey does not clear activeKeyId if removing non-active key`() {
        val key1 = manager.addKey(Provider.ANTHROPIC, "Key 1", "sk-ant-api03-test1")
        val key2 = manager.addKey(Provider.ANTHROPIC, "Key 2", "sk-ant-api03-test2")
        manager.setActiveKey(key1.id)

        manager.removeKey(key2.id)

        val state = manager.loadOrDefault()
        assertEquals(key1.id, state.activeKeyId)
    }

    @Test
    fun `setActiveKey updates state`() {
        val key = manager.addKey(Provider.ANTHROPIC, "Key 1", "sk-ant-api03-test1")

        manager.setActiveKey(key.id)

        val state = manager.loadOrDefault()
        assertEquals(key.id, state.activeKeyId)
    }

    @Test
    fun `setActiveKey throws when key ID does not exist`() {
        val exception = try {
            manager.setActiveKey("nonexistent-key-id")
            null
        } catch (e: IllegalArgumentException) {
            e
        }

        assertNotNull(exception)
        assertTrue(exception!!.message!!.contains("not found in wallet"))
    }

    @Test
    fun `addKey throws when rawKey is blank`() {
        val exception = try {
            manager.addKey(Provider.ANTHROPIC, "Test Key", "")
            null
        } catch (e: IllegalArgumentException) {
            e
        }

        assertNotNull(exception)
        assertTrue(exception!!.message!!.contains("API key cannot be blank"))
    }

    @Test
    fun `addKey throws when rawKey is whitespace only`() {
        val exception = try {
            manager.addKey(Provider.ANTHROPIC, "Test Key", "   ")
            null
        } catch (e: IllegalArgumentException) {
            e
        }

        assertNotNull(exception)
        assertTrue(exception!!.message!!.contains("API key cannot be blank"))
    }

    @Test
    fun `addKey throws when label is blank`() {
        val exception = try {
            manager.addKey(Provider.ANTHROPIC, "", "sk-ant-api03-test")
            null
        } catch (e: IllegalArgumentException) {
            e
        }

        assertNotNull(exception)
        assertTrue(exception!!.message!!.contains("Label cannot be blank"))
    }

    @Test
    fun `addKey throws when label is whitespace only`() {
        val exception = try {
            manager.addKey(Provider.ANTHROPIC, "   ", "sk-ant-api03-test")
            null
        } catch (e: IllegalArgumentException) {
            e
        }

        assertNotNull(exception)
        assertTrue(exception!!.message!!.contains("Label cannot be blank"))
    }

    @Test
    fun `setChatModel updates state`() {
        manager.setChatModel("claude-opus-4-5-20251101")

        val state = manager.loadOrDefault()
        assertEquals("claude-opus-4-5-20251101", state.chatModelId)
    }

    @Test
    fun `setActionModel updates state`() {
        manager.setActionModel("claude-sonnet-4-5-20250929")

        val state = manager.loadOrDefault()
        assertEquals("claude-sonnet-4-5-20250929", state.actionModelId)
    }

    @Test
    fun `activeConfig returns null when no active key`() {
        assertNull(manager.activeConfig())
    }

    @Test
    fun `activeConfig returns correct ProviderConfig when active key set`() {
        val key = manager.addKey(Provider.ANTHROPIC, "Key 1", "sk-ant-api03-test1")
        manager.setActiveKey(key.id)

        val config = manager.activeConfig()
        assertNotNull(config)
        assertEquals(Provider.ANTHROPIC, config!!.provider)
        assertEquals("sk-ant-api03-test1", config.headers["x-api-key"])
    }

    @Test
    fun `migrateFromLegacy creates wallet entry with Anthropic provider`() {
        val token = "sk-ant-api03-legacy123"

        val key = manager.migrateFromLegacy(token, Provider.ANTHROPIC, null)

        assertEquals(Provider.ANTHROPIC, key.provider)
        assertEquals("Imported Key", key.label)
        assertTrue(key.id.isNotBlank())
        assertTrue(key.addedAt > 0)

        // Verify raw key stored
        assertEquals(token, keyStore.get(key.id))

        // Verify set as active
        val state = manager.loadOrDefault()
        assertEquals(key.id, state.activeKeyId)

        // Verify default models set for Anthropic
        assertEquals(ModelConfig.CHAT_MODEL, state.chatModelId)
        assertEquals(ModelConfig.ACTION_MODEL, state.actionModelId)
    }

    @Test
    fun `migrateFromLegacy creates wallet entry with OpenRouter provider`() {
        val token = "sk-or-legacy123"

        val key = manager.migrateFromLegacy(token, Provider.OPENROUTER, null)

        assertEquals(Provider.OPENROUTER, key.provider)

        // Verify default models set for OpenRouter
        val state = manager.loadOrDefault()
        assertEquals(ModelConfig.OPENROUTER_CHAT_MODEL, state.chatModelId)
        assertEquals(ModelConfig.OPENROUTER_ACTION_MODEL, state.actionModelId)
    }

    @Test
    fun `migrateFromLegacy creates wallet entry with OpenAI provider`() {
        val token = "sk-legacy123"

        val key = manager.migrateFromLegacy(token, Provider.OPENAI, null)

        assertEquals(Provider.OPENAI, key.provider)

        // Verify default models set for OpenAI
        val state = manager.loadOrDefault()
        assertEquals(ModelConfig.OPENAI_CHAT_MODEL, state.chatModelId)
        assertEquals(ModelConfig.OPENAI_ACTION_MODEL, state.actionModelId)
    }

    @Test
    fun `migrateFromLegacy auto-detects provider from key prefix when provider is null`() {
        val anthropicKey = manager.migrateFromLegacy("sk-ant-api03-test", null, null)
        assertEquals(Provider.ANTHROPIC, anthropicKey.provider)

        // Reset for next test
        setup()

        val openRouterKey = manager.migrateFromLegacy("sk-or-test", null, null)
        assertEquals(Provider.OPENROUTER, openRouterKey.provider)

        // Reset for next test
        setup()

        val openAiKey = manager.migrateFromLegacy("sk-test", null, null)
        assertEquals(Provider.OPENAI, openAiKey.provider)
    }

    @Test
    fun `migrateFromLegacy with oauth authKind suggests OpenAI provider`() {
        val token = "sess-oauth-token-123"

        val key = manager.migrateFromLegacy(token, null, "oauth")

        // When authKind is oauth, should default to OpenAI
        assertEquals(Provider.OPENAI, key.provider)

        val state = manager.loadOrDefault()
        assertEquals(ModelConfig.OPENAI_CHAT_MODEL, state.chatModelId)
        assertEquals(ModelConfig.OPENAI_ACTION_MODEL, state.actionModelId)
    }

    // ========== Model Catalog Tests ==========

    @Test
    fun `chatModelsForProvider returns Anthropic models`() {
        val models = ModelConfig.chatModelsForProvider(Provider.ANTHROPIC)

        assertTrue(models.contains("claude-sonnet-4-5-20250929"))
        assertTrue(models.contains("claude-opus-4-6"))
        assertTrue(models.contains("claude-haiku-3-5-20241022"))
    }

    @Test
    fun `chatModelsForProvider returns OpenRouter models`() {
        val models = ModelConfig.chatModelsForProvider(Provider.OPENROUTER)

        assertTrue(models.contains("anthropic/claude-sonnet-4.5"))
        assertTrue(models.contains("anthropic/claude-opus-4.5"))
        assertTrue(models.contains("openai/gpt-4o"))
    }

    @Test
    fun `chatModelsForProvider returns OpenAI models`() {
        val models = ModelConfig.chatModelsForProvider(Provider.OPENAI)

        assertTrue(models.contains("gpt-4o"))
        assertTrue(models.contains("gpt-4o-mini"))
        assertFalse(models.contains("o1"))
    }

    @Test
    fun `actionModelsForProvider returns Anthropic models above floor`() {
        val models = ModelConfig.actionModelsForProvider(Provider.ANTHROPIC)

        // Model floor: only Sonnet-tier and above
        assertTrue(models.contains("claude-sonnet-4-5-20250929"))
        assertFalse("Haiku should not be in action models (below floor)", models.contains("claude-haiku-3-5-20241022"))
        models.forEach { assertTrue("$it should be above floor", ModelConfig.isModelAboveFloor(Provider.ANTHROPIC, it)) }
    }

    @Test
    fun `actionModelsForProvider returns OpenRouter models above floor`() {
        val models = ModelConfig.actionModelsForProvider(Provider.OPENROUTER)

        assertTrue(models.contains("anthropic/claude-sonnet-4.5"))
        assertFalse("Haiku should not be in action models (below floor)", models.contains("anthropic/claude-haiku-4.5"))
    }

    @Test
    fun `actionModelsForProvider returns OpenAI models above floor`() {
        val models = ModelConfig.actionModelsForProvider(Provider.OPENAI)

        assertTrue(models.contains("gpt-4o"))
        assertFalse("GPT-4o-mini should not be in action models (below floor)", models.contains("gpt-4o-mini"))
    }

    @Test
    fun `defaultChatModel returns correct model for each provider`() {
        assertEquals(ModelConfig.CHAT_MODEL, ModelConfig.defaultChatModel(Provider.ANTHROPIC))
        assertEquals(ModelConfig.OPENROUTER_CHAT_MODEL, ModelConfig.defaultChatModel(Provider.OPENROUTER))
        assertEquals(ModelConfig.OPENAI_CHAT_MODEL, ModelConfig.defaultChatModel(Provider.OPENAI))
    }

    @Test
    fun `defaultActionModel returns correct model for each provider`() {
        assertEquals(ModelConfig.ACTION_MODEL, ModelConfig.defaultActionModel(Provider.ANTHROPIC))
        assertEquals(ModelConfig.OPENROUTER_ACTION_MODEL, ModelConfig.defaultActionModel(Provider.OPENROUTER))
        assertEquals(ModelConfig.OPENAI_ACTION_MODEL, ModelConfig.defaultActionModel(Provider.OPENAI))
    }
}

/**
 * In-memory implementation of KeyStore for testing.
 * No Android dependencies, no persistence.
 */
class InMemoryKeyStore : KeyStore {
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

/**
 * In-memory implementation of WalletStorage for testing.
 * No Android dependencies, no persistence.
 */
class InMemoryWalletStorage : WalletStorage {
    private var state: WalletState? = null

    override fun loadState(): WalletState? = state

    override fun saveState(state: WalletState) {
        this.state = state
    }
}

// ========== Wallet Hardening Tests (#505, #507) ==========

class WalletHardeningTest {

    private lateinit var keyStore: InMemoryKeyStore
    private lateinit var storage: InMemoryWalletStorage
    private lateinit var manager: WalletManager

    @Before
    fun setup() {
        keyStore = InMemoryKeyStore()
        storage = InMemoryWalletStorage()
        manager = WalletManager(storage, keyStore)
    }

    @Test
    fun `loadOrDefault cleans up orphaned key entries missing from KeyStore`() {
        // Simulate state with a key entry but no corresponding KeyStore entry
        // (as if keyStore.put() failed after saveState() succeeded)
        val orphanedState = WalletState(
            keys = listOf(
                WalletKey("orphan-id", Provider.ANTHROPIC, "Orphan Key", 1000L),
                WalletKey("valid-id", Provider.OPENAI, "Valid Key", 2000L)
            ),
            activeKeyId = "orphan-id",
            chatModelId = ModelConfig.CHAT_MODEL,
            actionModelId = ModelConfig.ACTION_MODEL
        )
        storage.saveState(orphanedState)
        keyStore.put("valid-id", "sk-valid-key")
        // Note: "orphan-id" is NOT in keyStore

        val state = manager.loadOrDefault()

        // Orphaned entry should be removed
        assertEquals(1, state.keys.size)
        assertEquals("valid-id", state.keys[0].id)
        // Active key was orphaned, should be cleared
        assertNull(state.activeKeyId)

        // Verify cleaned state was persisted
        val reloaded = WalletManager(storage, keyStore).loadOrDefault()
        assertEquals(1, reloaded.keys.size)
    }

    @Test
    fun `loadOrDefault preserves activeKeyId when it is not orphaned`() {
        val state = WalletState(
            keys = listOf(
                WalletKey("orphan-id", Provider.ANTHROPIC, "Orphan", 1000L),
                WalletKey("active-id", Provider.OPENAI, "Active", 2000L)
            ),
            activeKeyId = "active-id",
            chatModelId = ModelConfig.CHAT_MODEL,
            actionModelId = ModelConfig.ACTION_MODEL
        )
        storage.saveState(state)
        keyStore.put("active-id", "sk-active-key")

        val loaded = manager.loadOrDefault()

        assertEquals(1, loaded.keys.size)
        assertEquals("active-id", loaded.activeKeyId)
    }

    @Test
    fun `loadOrDefault does not modify state when no orphans exist`() {
        val state = WalletState(
            keys = listOf(WalletKey("id1", Provider.ANTHROPIC, "Key 1", 1000L)),
            activeKeyId = "id1",
            chatModelId = ModelConfig.CHAT_MODEL,
            actionModelId = ModelConfig.ACTION_MODEL
        )
        storage.saveState(state)
        keyStore.put("id1", "sk-key")

        val loaded = manager.loadOrDefault()

        assertEquals(state, loaded)
    }

    @Test
    fun `migrateFromLegacy is idempotent - returns existing key if wallet non-empty`() {
        // First migration
        val key1 = manager.migrateFromLegacy("sk-ant-api03-first", Provider.ANTHROPIC, null)

        // Second migration attempt with different token
        val key2 = manager.migrateFromLegacy("sk-ant-api03-second", null, null)

        // Should return the existing key, not create a duplicate
        assertEquals(key1.id, key2.id)

        // Wallet should still have exactly 1 key
        val state = manager.loadOrDefault()
        assertEquals(1, state.keys.size)

        // Second token should NOT be in keyStore
        assertNull(keyStore.get(key2.id + "-should-not-exist"))
    }

    @Test
    fun `migrateFromLegacy works normally on empty wallet`() {
        val key = manager.migrateFromLegacy("sk-ant-api03-test", Provider.ANTHROPIC, null)

        val state = manager.loadOrDefault()
        assertEquals(1, state.keys.size)
        assertEquals(key.id, state.activeKeyId)
        assertEquals(Provider.ANTHROPIC, key.provider)
    }

    @Test
    fun `concurrent addKey calls maintain consistency`() {
        val threads = (1..10).map { i ->
            thread {
                manager.addKey(Provider.ANTHROPIC, "Key $i", "sk-ant-api03-test$i")
            }
        }
        threads.forEach { it.join() }

        val state = manager.loadOrDefault()
        assertEquals(10, state.keys.size)
        assertEquals(10, state.keys.map { it.id }.distinct().size)
    }

    @Test
    fun `concurrent addKey and removeKey do not corrupt state`() {
        // Add 5 keys first
        val keys = (1..5).map { i ->
            manager.addKey(Provider.ANTHROPIC, "Key $i", "sk-ant-api03-test$i")
        }

        // Concurrently add 5 more and remove the original 5
        val addThreads = (6..10).map { i ->
            thread {
                manager.addKey(Provider.ANTHROPIC, "Key $i", "sk-ant-api03-test$i")
            }
        }
        val removeThreads = keys.map { key ->
            thread {
                manager.removeKey(key.id)
            }
        }
        (addThreads + removeThreads).forEach { it.join() }

        val state = manager.loadOrDefault()
        // Should have exactly 5 keys (the newly added ones)
        assertEquals(5, state.keys.size)
    }
}
