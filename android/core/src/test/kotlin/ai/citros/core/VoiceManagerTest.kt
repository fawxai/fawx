package ai.citros.core

import android.content.Context
import android.content.SharedPreferences
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.emptyFlow
import kotlinx.coroutines.test.runTest
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment

/**
 * Tests for [VoiceManager] — provider registration, switching, persistence,
 * availability checks, and release lifecycle.
 *
 * Uses Robolectric for SharedPreferences support.
 */
@RunWith(RobolectricTestRunner::class)
class VoiceManagerTest {

    private lateinit var context: Context
    private lateinit var prefs: SharedPreferences

    private lateinit var stt1: FakeSttProvider
    private lateinit var stt2: FakeSttProvider
    private lateinit var tts1: FakeTtsProvider
    private lateinit var tts2: FakeTtsProvider

    @Before
    fun setUp() {
        context = RuntimeEnvironment.getApplication()
        prefs = context.getSharedPreferences("test_voice_prefs", Context.MODE_PRIVATE)
        prefs.edit().clear().commit()

        stt1 = FakeSttProvider("stt-1", "STT One", available = true)
        stt2 = FakeSttProvider("stt-2", "STT Two", available = true)
        tts1 = FakeTtsProvider("tts-1", "TTS One", available = true)
        tts2 = FakeTtsProvider("tts-2", "TTS Two", available = true)
    }

    private fun createManager(
        sttList: List<SpeechToTextProvider> = listOf(stt1, stt2),
        ttsList: List<TextToSpeechProvider> = listOf(tts1, tts2)
    ) = VoiceManager(
        context = context,
        sttProviders = sttList,
        ttsProviders = ttsList,
        prefs = prefs
    )

    // ========== Registration ==========

    @Test
    fun `providers are registered`() {
        val manager = createManager()
        assertEquals(2, manager.sttProviders.size)
        assertEquals(2, manager.ttsProviders.size)
    }

    @Test
    fun `default active provider is first in list`() {
        val manager = createManager()
        assertEquals("stt-1", manager.activeStt.value.providerId)
        assertEquals("tts-1", manager.activeTts.value.providerId)
    }

    // ========== Switching ==========

    @Test
    fun `switchStt changes active provider`() = runTest {
        val manager = createManager()
        val result = manager.switchStt("stt-2")
        assertTrue(result.isSuccess)
        assertEquals("stt-2", manager.activeStt.value.providerId)
    }

    @Test
    fun `switchTts changes active provider`() = runTest {
        val manager = createManager()
        val result = manager.switchTts("tts-2")
        assertTrue(result.isSuccess)
        assertEquals("tts-2", manager.activeTts.value.providerId)
    }

    @Test
    fun `switchStt fails for unknown provider`() = runTest {
        val manager = createManager()
        val result = manager.switchStt("nonexistent")
        assertTrue(result.isFailure)
        assertTrue(result.exceptionOrNull() is IllegalArgumentException)
        // Active unchanged
        assertEquals("stt-1", manager.activeStt.value.providerId)
    }

    @Test
    fun `switchTts fails for unknown provider`() = runTest {
        val manager = createManager()
        val result = manager.switchTts("nonexistent")
        assertTrue(result.isFailure)
        assertTrue(result.exceptionOrNull() is IllegalArgumentException)
    }

    @Test
    fun `switchStt fails for unavailable provider`() = runTest {
        stt2.available = false
        val manager = createManager()
        val result = manager.switchStt("stt-2")
        assertTrue(result.isFailure)
        assertTrue(result.exceptionOrNull() is IllegalStateException)
        assertEquals("stt-1", manager.activeStt.value.providerId)
    }

    @Test
    fun `switchTts fails for unavailable provider`() = runTest {
        tts2.available = false
        val manager = createManager()
        val result = manager.switchTts("tts-2")
        assertTrue(result.isFailure)
        assertTrue(result.exceptionOrNull() is IllegalStateException)
    }

    // ========== Persistence ==========

    @Test
    fun `switchStt persists selection`() = runTest {
        val manager = createManager()
        manager.switchStt("stt-2")
        assertEquals("stt-2", prefs.getString(VoiceManager.KEY_ACTIVE_STT, null))
    }

    @Test
    fun `switchTts persists selection`() = runTest {
        val manager = createManager()
        manager.switchTts("tts-2")
        assertEquals("tts-2", prefs.getString(VoiceManager.KEY_ACTIVE_TTS, null))
    }

    @Test
    fun `restores persisted STT selection`() {
        prefs.edit().putString(VoiceManager.KEY_ACTIVE_STT, "stt-2").commit()
        val manager = createManager()
        assertEquals("stt-2", manager.activeStt.value.providerId)
    }

    @Test
    fun `restores persisted TTS selection`() {
        prefs.edit().putString(VoiceManager.KEY_ACTIVE_TTS, "tts-2").commit()
        val manager = createManager()
        assertEquals("tts-2", manager.activeTts.value.providerId)
    }

    @Test
    fun `falls back to first provider if persisted ID not found`() {
        prefs.edit().putString(VoiceManager.KEY_ACTIVE_STT, "deleted-provider").commit()
        val manager = createManager()
        assertEquals("stt-1", manager.activeStt.value.providerId)
    }

    // ========== Settings ==========

    @Test
    fun `autoSpeakResponses defaults to false`() {
        val manager = createManager()
        assertFalse(manager.autoSpeakResponses.value)
    }

    @Test
    fun `autoSendAfterVoice defaults to false`() {
        val manager = createManager()
        assertFalse(manager.autoSendAfterVoice.value)
    }

    @Test
    fun `setAutoSpeakResponses persists`() {
        val manager = createManager()
        manager.setAutoSpeakResponses(true)
        assertTrue(manager.autoSpeakResponses.value)
        assertTrue(prefs.getBoolean(VoiceManager.KEY_AUTO_SPEAK, false))
    }

    @Test
    fun `setAutoSendAfterVoice persists`() {
        val manager = createManager()
        manager.setAutoSendAfterVoice(true)
        assertTrue(manager.autoSendAfterVoice.value)
        assertTrue(prefs.getBoolean(VoiceManager.KEY_AUTO_SEND, false))
    }

    @Test
    fun `restores persisted auto settings`() {
        prefs.edit()
            .putBoolean(VoiceManager.KEY_AUTO_SPEAK, true)
            .putBoolean(VoiceManager.KEY_AUTO_SEND, true)
            .commit()
        val manager = createManager()
        assertTrue(manager.autoSpeakResponses.value)
        assertTrue(manager.autoSendAfterVoice.value)
    }

    // ========== Release ==========

    @Test
    fun `release delegates to all providers`() {
        val manager = createManager()
        manager.release()
        assertTrue(stt1.released)
        assertTrue(stt2.released)
        assertTrue(tts1.released)
        assertTrue(tts2.released)
    }

    @Test(expected = IllegalArgumentException::class)
    fun `empty STT provider list throws`() {
        createManager(sttList = emptyList())
    }

    @Test(expected = IllegalArgumentException::class)
    fun `empty TTS provider list throws`() {
        createManager(ttsList = emptyList())
    }

    // ========== Fakes ==========

    private class FakeSttProvider(
        override val providerId: String,
        override val displayName: String,
        var available: Boolean = true
    ) : SpeechToTextProvider {
        var released = false
        override val requiresNetwork = false
        override val isAvailable get() = available
        override suspend fun initialize(context: Context) {}
        override fun startListening(): Flow<SpeechEvent> = emptyFlow()
        override fun stopListening() {}
        override fun cancel() {}
        override fun release() { released = true }
    }

    private class FakeTtsProvider(
        override val providerId: String,
        override val displayName: String,
        var available: Boolean = true
    ) : TextToSpeechProvider {
        var released = false
        override val requiresNetwork = false
        override val isAvailable get() = available
        override val isSpeaking = false
        override suspend fun initialize(context: Context) {}
        override suspend fun speak(text: String, options: TtsOptions) {}
        override fun stop() {}
        override fun release() { released = true }
    }
}
