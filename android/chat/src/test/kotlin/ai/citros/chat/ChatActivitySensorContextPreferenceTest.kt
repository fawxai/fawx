package ai.citros.chat

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import ai.citros.core.ChatResponse
import ai.citros.core.PhoneAgentApi
import ai.citros.core.Provider
import ai.citros.core.ProviderClient
import ai.citros.core.SensorProvider
import ai.citros.core.Tool
import kotlinx.coroutines.test.runTest
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertNotNull
import kotlin.test.assertNull

@RunWith(RobolectricTestRunner::class)
class ChatActivitySensorContextPreferenceTest {

    private val context: Context
        get() = ApplicationProvider.getApplicationContext()

    @Before
    fun clearPrefs() {
        context.getSharedPreferences(CITROS_PREFS, Context.MODE_PRIVATE).edit().clear().commit()
    }

    @Test
    fun `applySensorContextPreference default off keeps sensor provider null`() {
        val prefs = context.getSharedPreferences(CITROS_PREFS, Context.MODE_PRIVATE)
        val viewModel = ChatViewModel()

        applySensorContextPreference(
            prefs = prefs,
            appContext = context.applicationContext,
            viewModel = viewModel
        )

        assertNull(readSensorProvider(viewModel))
    }

    @Test
    fun `sensor context preference listener toggles provider at runtime`() {
        val prefs = context.getSharedPreferences(CITROS_PREFS, Context.MODE_PRIVATE)
        val viewModel = ChatViewModel()
        val listener = createSensorContextPreferenceChangeListener(
            prefs = prefs,
            appContext = context.applicationContext,
            viewModel = viewModel
        )

        prefs.edit().putBoolean(PREF_SENSOR_CONTEXT_ENABLED, true).commit()
        listener.onSharedPreferenceChanged(prefs, PREF_SENSOR_CONTEXT_ENABLED)

        val enabledProvider = readSensorProvider(viewModel)
        assertNotNull(enabledProvider)
        assertIs<AndroidSensorProvider>(enabledProvider)

        prefs.edit().putBoolean(PREF_SENSOR_CONTEXT_ENABLED, false).commit()
        listener.onSharedPreferenceChanged(prefs, PREF_SENSOR_CONTEXT_ENABLED)

        assertNull(readSensorProvider(viewModel))
    }

    @Test
    fun `sensor context preference listener ignores unrelated keys`() {
        val prefs = context.getSharedPreferences(CITROS_PREFS, Context.MODE_PRIVATE)
        val viewModel = ChatViewModel()
        val listener = createSensorContextPreferenceChangeListener(
            prefs = prefs,
            appContext = context.applicationContext,
            viewModel = viewModel
        )

        prefs.edit().putBoolean(PREF_SENSOR_CONTEXT_ENABLED, true).commit()
        listener.onSharedPreferenceChanged(prefs, "unrelated_pref_key")

        assertNull(readSensorProvider(viewModel))
    }

    @Test
    fun `prefs load plus listener toggle off leaves PhoneAgentApi prompt without sensor metadata`() = runTest {
        val prefs = context.getSharedPreferences(CITROS_PREFS, Context.MODE_PRIVATE)
        val viewModel = ChatViewModel()

        prefs.edit().putBoolean(PREF_SENSOR_CONTEXT_ENABLED, true).commit()
        applySensorContextPreference(
            prefs = prefs,
            appContext = context.applicationContext,
            viewModel = viewModel
        )
        assertNotNull(readSensorProvider(viewModel))

        val listener = createSensorContextPreferenceChangeListener(
            prefs = prefs,
            appContext = context.applicationContext,
            viewModel = viewModel
        )
        prefs.edit().putBoolean(PREF_SENSOR_CONTEXT_ENABLED, false).commit()
        listener.onSharedPreferenceChanged(prefs, PREF_SENSOR_CONTEXT_ENABLED)

        val providerAfterToggleOff = readSensorProvider(viewModel)
        assertNull(providerAfterToggleOff)

        val promptClient = RecordingPromptProviderClient()
        val agent = PhoneAgentApi(
            chatClient = promptClient,
            actionClient = promptClient,
            sensorProvider = providerAfterToggleOff as? SensorProvider
        ).also { it.phoneControlOverride = true }

        agent.sendMessage("Open Settings", screenContent = null, isActionLoop = false)

        assertNotNull(promptClient.lastSystemPrompt)
        assertFalse(promptClient.lastSystemPrompt!!.contains("Device:"))
    }

    private fun readSensorProvider(viewModel: ChatViewModel): Any? {
        val field = ChatViewModel::class.java.getDeclaredField("sensorProvider")
        field.isAccessible = true
        return field.get(viewModel)
    }

    private class RecordingPromptProviderClient : ProviderClient {
        override val provider: Provider = Provider.ANTHROPIC
        override val modelId: String? = null
        var lastSystemPrompt: String? = null

        override suspend fun chat(conversation: ai.citros.core.Conversation): Result<String> = Result.success("chat")

        override suspend fun chatWithTools(
            messages: List<ai.citros.core.Message>,
            systemPrompt: String?,
            tools: List<Tool>,
            tokenLimit: Int?
        ): Result<ChatResponse> {
            lastSystemPrompt = systemPrompt
            return Result.success(ChatResponse(text = "ok", toolCalls = emptyList(), stopReason = "end_turn"))
        }

        override suspend fun describeImage(base64Image: String, prompt: String, maxTokens: Int): Result<String> {
            return Result.success("desc")
        }
    }
}
