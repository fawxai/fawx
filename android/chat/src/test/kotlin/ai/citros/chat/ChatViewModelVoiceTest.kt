package ai.citros.chat

import ai.citros.core.SpeechEvent
import ai.citros.core.SpeechToTextProvider
import ai.citros.core.TextToSpeechProvider
import ai.citros.core.TtsOptions
import ai.citros.core.VoiceManager
import android.content.Context
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.emptyFlow
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.setMain
import kotlinx.coroutines.test.resetMain
import org.junit.After
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

@OptIn(ExperimentalCoroutinesApi::class)
@RunWith(RobolectricTestRunner::class)
class ChatViewModelVoiceTest {

    private lateinit var viewModel: ChatViewModel
    private val testDispatcher = StandardTestDispatcher()

    @Before
    fun setUp() {
        Dispatchers.setMain(testDispatcher)
        viewModel = ChatViewModel()
    }

    @After
    fun tearDown() {
        Dispatchers.resetMain()
    }

    @Test
    fun `setVoiceManager sets voiceReady to true`() {
        assertFalse(viewModel.voiceReady.value)
        assertNull(viewModel.voiceManager.value)

        val vm = createMockVoiceManager()
        viewModel.setVoiceManager(vm)

        assertTrue(viewModel.voiceReady.value)
        assertNotNull(viewModel.voiceManager.value)
    }

    @Test
    fun `releaseVoiceManager resets state`() {
        val vm = createMockVoiceManager()
        viewModel.setVoiceManager(vm)
        assertTrue(viewModel.voiceReady.value)

        viewModel.releaseVoiceManager()

        assertFalse(viewModel.voiceReady.value)
        assertNull(viewModel.voiceManager.value)
    }

    @Test
    fun `releaseVoiceManager called twice does not throw`() {
        val vm = createMockVoiceManager()
        viewModel.setVoiceManager(vm)

        viewModel.releaseVoiceManager()
        viewModel.releaseVoiceManager()

        assertFalse(viewModel.voiceReady.value)
        assertNull(viewModel.voiceManager.value)
    }

    @Test
    fun `voiceReady is false initially`() {
        assertFalse(viewModel.voiceReady.value)
    }

    @Test
    fun `voiceManager is null initially`() {
        assertNull(viewModel.voiceManager.value)
    }

    private fun createMockVoiceManager(): VoiceManager {
        val stt = object : SpeechToTextProvider {
            override val providerId = "test-stt"
            override val displayName = "Test STT"
            override val requiresNetwork = false
            override val isAvailable = true
            override suspend fun initialize(context: Context) {}
            override fun startListening(): Flow<SpeechEvent> = emptyFlow()
            override fun stopListening() {}
            override fun cancel() {}
            override fun release() {}
        }
        val tts = object : TextToSpeechProvider {
            override val providerId = "test-tts"
            override val displayName = "Test TTS"
            override val requiresNetwork = false
            override val isAvailable = true
            override val isSpeaking = false
            override suspend fun initialize(context: Context) {}
            override suspend fun speak(text: String, options: TtsOptions) {}
            override fun stop() {}
            override fun release() {}
        }
        val context = androidx.test.core.app.ApplicationProvider.getApplicationContext<android.app.Application>()
        return VoiceManager(
            context = context,
            sttProviders = listOf(stt),
            ttsProviders = listOf(tts)
        )
    }
}
