package ai.citros.chat

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import ai.citros.core.ChatResponse
import ai.citros.core.PhoneAgentApi
import ai.citros.core.Provider
import ai.citros.core.ProviderClient
import ai.citros.core.SensorProvider
import ai.citros.core.Tool
import ai.citros.core.SensorContext
import kotlinx.coroutines.test.runTest
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

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
    fun `sensor context toggle applies immediately to next prompt build without screen re-entry`() = runTest {
        val prefs = context.getSharedPreferences(CITROS_PREFS, Context.MODE_PRIVATE)
        val viewModel = ChatViewModel()

        prefs.edit().putBoolean(PREF_SENSOR_CONTEXT_ENABLED, true).commit()
        applySensorContextPreference(
            prefs = prefs,
            appContext = context.applicationContext,
            viewModel = viewModel
        )
        assertNotNull(readSensorProvider(viewModel))

        // Step 1: Build prompt with sensor context enabled + deterministic provider
        val deterministicSensorProvider = object : SensorProvider {
            override suspend fun snapshot(): SensorContext = SensorContext(batteryPercent = 87)
        }
        viewModel.setSensorProvider(deterministicSensorProvider)

        val promptClient = RecordingPromptProviderClient()
        val enabledAgent = PhoneAgentApi(
            chatClient = promptClient,
            actionClient = promptClient,
            sensorProvider = readSensorProvider(viewModel) as? SensorProvider
        ).also { it.phoneControlOverride = true }

        enabledAgent.sendMessage("Open Settings", screenContent = null, isActionLoop = false)

        val promptWithSensors = promptClient.systemPrompts.lastOrNull()
        assertNotNull(promptWithSensors)
        assertTrue(promptWithSensors.contains("Device: battery=87%"))

        // Step 2: Toggle off and verify next prompt excludes sensor context
        val listener = createSensorContextPreferenceChangeListener(
            prefs = prefs,
            appContext = context.applicationContext,
            viewModel = viewModel
        )
        prefs.edit().putBoolean(PREF_SENSOR_CONTEXT_ENABLED, false).commit()
        listener.onSharedPreferenceChanged(prefs, PREF_SENSOR_CONTEXT_ENABLED)

        val providerAfterToggleOff = readSensorProvider(viewModel)
        assertNull(providerAfterToggleOff)

        val disabledAgent = PhoneAgentApi(
            chatClient = promptClient,
            actionClient = promptClient,
            sensorProvider = providerAfterToggleOff as? SensorProvider
        ).also { it.phoneControlOverride = true }

        disabledAgent.sendMessage("Open Settings again", screenContent = null, isActionLoop = false)

        val promptAfterToggle = promptClient.systemPrompts.lastOrNull()
        assertNotNull(promptAfterToggle)
        assertFalse(promptAfterToggle.contains("Device:"))
    }

    private fun readSensorProvider(viewModel: ChatViewModel): Any? {
        val field = ChatViewModel::class.java.getDeclaredField("sensorProvider")
        field.isAccessible = true
        return field.get(viewModel)
    }

    private class RecordingPromptProviderClient : ProviderClient {
        override val provider: Provider = Provider.ANTHROPIC
        override val modelId: String? = null
        val systemPrompts: MutableList<String> = mutableListOf()

        override suspend fun chat(conversation: ai.citros.core.Conversation): Result<String> = Result.success("chat")

        override suspend fun chatWithTools(
            messages: List<ai.citros.core.Message>,
            systemPrompt: String?,
            tools: List<Tool>,
            tokenLimit: Int?
        ): Result<ChatResponse> {
            if (systemPrompt != null) {
                systemPrompts.add(systemPrompt)
            }
            return Result.success(ChatResponse(text = "ok", toolCalls = emptyList(), stopReason = "end_turn"))
        }

        override suspend fun describeImage(base64Image: String, prompt: String, maxTokens: Int): Result<String> {
            return Result.success("desc")
        }
    }
}
